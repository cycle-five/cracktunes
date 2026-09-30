//! One `watch` channel per guild, alive only while someone is watching.
//! Its task asks the source for the view every tick and publishes only when
//! it changed. Polling, deliberately: the queue changes from a dozen places,
//! and "remember to notify" at each is the bug class #434 removed.

use crate::view::QueueView;
use serenity::all::GuildId;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex};

/// How often a watched guild's queue is read.
pub const TICK: Duration = Duration::from_secs(1);
/// How long a guild's poller outlives its last watcher (a reload is cheap).
pub const LINGER: Duration = Duration::from_secs(30);

/// Where views come from.
pub trait ViewSource: Send + Sync + 'static {
    fn view(&self, guild_id: GuildId) -> impl Future<Output = QueueView> + Send;
}

pub struct Hub<S: ViewSource> {
    source: Arc<S>,
    guilds: Mutex<HashMap<GuildId, watch::Sender<Arc<QueueView>>>>,
    tick: Duration,
    linger: Duration,
}

impl<S: ViewSource> Hub<S> {
    pub fn new(source: Arc<S>, tick: Duration, linger: Duration) -> Arc<Self> {
        Arc::new(Self {
            source,
            guilds: Mutex::new(HashMap::new()),
            tick,
            linger,
        })
    }

    /// Watch a guild. The first watcher starts its poller.
    pub async fn subscribe(self: &Arc<Self>, guild_id: GuildId) -> watch::Receiver<Arc<QueueView>> {
        if let Some(tx) = self.guilds.lock().await.get(&guild_id) {
            return tx.subscribe();
        }
        // Read outside the map lock: a slow read must not stall other guilds.
        let initial = Arc::new(self.source.view(guild_id).await);
        let mut guilds = self.guilds.lock().await;
        if let Some(tx) = guilds.get(&guild_id) {
            return tx.subscribe(); // another watcher raced us here
        }
        let (tx, rx) = watch::channel(initial);
        guilds.insert(guild_id, tx.clone());
        drop(guilds);
        tokio::spawn(self.clone().run(guild_id, tx));
        rx
    }

    /// Push a view now -- after a move, so every open tab follows at once.
    pub async fn publish(&self, guild_id: GuildId, view: QueueView) {
        if let Some(tx) = self.guilds.lock().await.get(&guild_id) {
            tx.send_if_modified(|cur| replace_if_changed(cur, view));
        }
    }

    async fn run(self: Arc<Self>, guild_id: GuildId, tx: watch::Sender<Arc<QueueView>>) {
        let mut interval = tokio::time::interval(self.tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await; // the first tick is immediate; the view is fresh
        let mut idle_since: Option<tokio::time::Instant> = None;
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                let since = *idle_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() < self.linger {
                    continue;
                }
                // Re-checked under the map lock, which `subscribe` also holds
                // while it calls `tx.subscribe()`: no watcher can slip in
                // between this check and the removal.
                let mut guilds = self.guilds.lock().await;
                if tx.receiver_count() == 0 {
                    guilds.remove(&guild_id);
                    return;
                }
            }
            idle_since = None;
            // Each read runs in its own task: a panic in the source must not
            // kill this poller and leave the map holding a dead sender.
            let source = self.source.clone();
            match tokio::spawn(async move { source.view(guild_id).await }).await {
                Ok(view) => {
                    tx.send_if_modified(|cur| replace_if_changed(cur, view));
                },
                // Keep the last view and keep polling.
                Err(e) => tracing::error!(guild = %guild_id, "dashboard view read failed: {e}"),
            }
        }
    }
}

fn replace_if_changed(cur: &mut Arc<QueueView>, view: QueueView) -> bool {
    if **cur == view {
        return false;
    }
    *cur = Arc::new(view);
    true
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::view::{QueueView, TrackView};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use uuid::Uuid;

    const G: GuildId = GuildId::new(1);

    struct Fake {
        view: Mutex<QueueView>,
        calls: AtomicUsize,
        /// The call number (1-based) that panics; 0 for never.
        panic_on: AtomicUsize,
    }

    impl ViewSource for Fake {
        async fn view(&self, _g: GuildId) -> QueueView {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            assert_ne!(
                n,
                self.panic_on.load(Ordering::SeqCst),
                "the source panicked"
            );
            self.view.lock().unwrap().clone()
        }
    }

    fn playing(n: u128) -> QueueView {
        let t = |id| TrackView {
            id: Uuid::from_u128(id),
            title: "t".into(),
            url: None,
            duration_secs: None,
            requester: None,
        };
        QueueView::Playing {
            now: t(n),
            upcoming: vec![],
            rev: n as u64,
        }
    }

    fn hub(view: QueueView) -> (Arc<Fake>, Arc<Hub<Fake>>) {
        let fake = Arc::new(Fake {
            view: Mutex::new(view),
            calls: AtomicUsize::new(0),
            panic_on: AtomicUsize::new(0),
        });
        let hub = Hub::new(
            fake.clone(),
            Duration::from_secs(1),
            Duration::from_secs(30),
        );
        (fake, hub)
    }

    async fn ticks(n: u32) {
        for _ in 0..n {
            tokio::time::advance(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_subscriber_starts_with_the_current_view() {
        let (_fake, hub) = hub(playing(1));
        let rx = hub.subscribe(G).await;
        assert_eq!(**rx.borrow(), playing(1));
    }

    #[tokio::test(start_paused = true)]
    async fn a_change_is_published_and_no_change_is_not() {
        let (fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        ticks(3).await;
        assert!(!rx.has_changed().unwrap(), "same view, no event");
        *fake.view.lock().unwrap() = playing(2);
        ticks(2).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(**rx.borrow_and_update(), playing(2));
    }

    #[tokio::test(start_paused = true)]
    async fn two_watchers_share_one_poller() {
        let (fake, hub) = hub(playing(1));
        let _a = hub.subscribe(G).await;
        let _b = hub.subscribe(G).await;
        let before = fake.calls.load(Ordering::SeqCst);
        ticks(5).await;
        let polled = fake.calls.load(Ordering::SeqCst) - before;
        assert!((4..=6).contains(&polled), "one poll per tick, got {polled}");
    }

    #[tokio::test(start_paused = true)]
    async fn polling_stops_after_the_last_watcher_lingers_out_and_restarts() {
        let (fake, hub) = hub(playing(1));
        let rx = hub.subscribe(G).await;
        drop(rx);
        ticks(35).await;
        let stopped_at = fake.calls.load(Ordering::SeqCst);
        // Longer than a linger: a poller that survived its own removal would
        // wake again within it.
        ticks(100).await;
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            stopped_at,
            "no polling with nobody watching"
        );
        let rx = hub.subscribe(G).await;
        assert_eq!(**rx.borrow(), playing(1));
        ticks(3).await;
        assert!(
            fake.calls.load(Ordering::SeqCst) > stopped_at + 1,
            "polling again"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn publish_reaches_watchers_at_once() {
        let (_fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        hub.publish(G, playing(3)).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(**rx.borrow(), playing(3));
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_read_keeps_the_last_view_and_polling_continues() {
        let (fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await; // call 1
        rx.borrow_and_update();
        fake.panic_on.store(3, Ordering::SeqCst); // the second poll
        ticks(5).await;
        assert!(
            fake.calls.load(Ordering::SeqCst) >= 5,
            "polling went on past the panic"
        );
        assert_eq!(**rx.borrow(), playing(1), "the last view is kept");
        *fake.view.lock().unwrap() = playing(2);
        ticks(2).await;
        assert_eq!(
            **rx.borrow_and_update(),
            playing(2),
            "later changes still publish"
        );
    }
}
