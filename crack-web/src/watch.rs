//! One `watch` channel per guild, alive only while someone is watching.
//! Its task asks the source for the view every tick and publishes only when
//! it changed. Polling, deliberately: the queue changes from a dozen places,
//! and "remember to notify" at each is the bug class #434 removed.
//!
//! Every read and every send happens in that one task, in order. A caller
//! that knows the queue just changed asks for an early read ([`Hub::refresh`])
//! rather than sending a view itself: a poll that began before the change
//! could otherwise land after it and put the old order back.

use crate::view::QueueView;
use serenity::all::GuildId;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex, Notify};

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
    guilds: Mutex<HashMap<GuildId, Watched>>,
    tick: Duration,
    linger: Duration,
}

/// A watched guild: its channel, and the bell that wakes its poller early.
struct Watched {
    tx: watch::Sender<Arc<QueueView>>,
    wake: Arc<Notify>,
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
        if let Some(w) = self.guilds.lock().await.get(&guild_id) {
            return w.tx.subscribe();
        }
        // Read outside the map lock: a slow read must not stall other guilds.
        let initial = Arc::new(self.source.view(guild_id).await);
        let mut guilds = self.guilds.lock().await;
        if let Some(w) = guilds.get(&guild_id) {
            return w.tx.subscribe(); // another watcher raced us here
        }
        let (tx, rx) = watch::channel(initial);
        let wake = Arc::new(Notify::new());
        guilds.insert(
            guild_id,
            Watched {
                tx: tx.clone(),
                wake: wake.clone(),
            },
        );
        drop(guilds);
        tokio::spawn(self.clone().run(guild_id, tx, wake));
        rx
    }

    /// Read the guild's view now rather than at the next tick -- after a
    /// move, so every open tab follows at once. A no-op with nobody watching.
    pub async fn refresh(&self, guild_id: GuildId) {
        if let Some(w) = self.guilds.lock().await.get(&guild_id) {
            // Stores a permit if the poller is mid-read, so the read that
            // answers this one starts after the call.
            w.wake.notify_one();
        }
    }

    async fn run(
        self: Arc<Self>,
        guild_id: GuildId,
        tx: watch::Sender<Arc<QueueView>>,
        wake: Arc<Notify>,
    ) {
        let mut interval = tokio::time::interval(self.tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await; // the first tick is immediate; the view is fresh
        let mut idle_since: Option<tokio::time::Instant> = None;
        loop {
            tokio::select! {
                _ = interval.tick() => {},
                () = wake.notified() => {},
            }
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
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    };
    use uuid::Uuid;

    const G: GuildId = GuildId::new(1);

    struct Fake {
        view: Mutex<QueueView>,
        calls: AtomicUsize,
        /// The call number (1-based) that panics; 0 for never.
        panic_on: AtomicUsize,
        /// Take a fifth of a tick to return what was read at the start.
        slow: AtomicBool,
    }

    impl ViewSource for Fake {
        async fn view(&self, _g: GuildId) -> QueueView {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            assert_ne!(
                n,
                self.panic_on.load(Ordering::SeqCst),
                "the source panicked"
            );
            let view = self.view.lock().unwrap().clone();
            if self.slow.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            view
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
            paused: false,
            looping: false,
        }
    }

    fn hub(view: QueueView) -> (Arc<Fake>, Arc<Hub<Fake>>) {
        let fake = Arc::new(Fake {
            view: Mutex::new(view),
            calls: AtomicUsize::new(0),
            panic_on: AtomicUsize::new(0),
            slow: AtomicBool::new(false),
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
    async fn a_refresh_reads_and_publishes_without_waiting_for_the_tick() {
        let (fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        let before = fake.calls.load(Ordering::SeqCst);
        *fake.view.lock().unwrap() = playing(3);
        let asked = tokio::time::Instant::now();
        hub.refresh(G).await;
        // The paused clock jumps to the next timer only once every task is
        // idle: a poller that waited for its tick would arrive a tick later.
        rx.changed().await.unwrap();
        assert!(asked.elapsed() < Duration::from_secs(1), "before the tick");
        assert_eq!(**rx.borrow(), playing(3));
        assert_eq!(fake.calls.load(Ordering::SeqCst), before + 1, "one read");
    }

    #[tokio::test(start_paused = true)]
    async fn a_refresh_during_a_read_reads_again_after_it() {
        let (fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        // A read in flight when the queue changes and the refresh is asked:
        // its old view must not be the last word.
        fake.slow.store(true, Ordering::SeqCst);
        let before = fake.calls.load(Ordering::SeqCst);
        while fake.calls.load(Ordering::SeqCst) == before {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // The poller is now inside a slow read of playing(1).
        *fake.view.lock().unwrap() = playing(2);
        hub.refresh(G).await;
        fake.slow.store(false, Ordering::SeqCst);
        let asked = tokio::time::Instant::now();
        tokio::time::timeout(Duration::from_millis(900), async {
            while **rx.borrow_and_update() != playing(2) {
                rx.changed().await.unwrap();
            }
        })
        .await
        .expect("the refreshed view, before the next tick");
        assert!(asked.elapsed() < Duration::from_secs(1));
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
