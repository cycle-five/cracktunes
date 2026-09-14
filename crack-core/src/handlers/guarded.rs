//! Songbird event handlers that survive their own panics.
//!
//! 🔑 songbird runs every handler's `act` inline, on the call's one event task,
//! and that task keeps the call's global handlers, every track's event store and
//! every track's state as plain locals (`driver/tasks/events.rs`). A panic in
//! any handler unwinds the task and drops all of it: the queue stops advancing,
//! because its own `End` handler is gone; autoplay, idle-disconnect and the
//! queue-message refresh go quiet; and nothing but the panic is ever logged.
//! v0.12.0's autoplay did exactly that on TuneTitan.
//!
//! [`add_global_handler`] and [`add_track_handler`] are the crate's only ways
//! to register a handler, and both wrap it so that a panic costs one event
//! instead of the call. `clippy.toml` bans songbird's own registration methods
//! everywhere else.

use ::serenity::async_trait;
use futures::FutureExt;
use songbird::{
    tracks::{TrackHandle, TrackResult},
    Call, Event, EventContext, EventHandler,
};
use std::{any::Any, panic::AssertUnwindSafe};

/// Register `handler` for `event` across `call`, guarded against its panics.
#[allow(clippy::disallowed_methods)]
pub fn add_global_handler<H: EventHandler + 'static>(call: &mut Call, event: Event, handler: H) {
    call.add_global_event(event, Guarded(handler));
}

/// Register `handler` for `event` on one track, guarded against its panics.
///
/// # Errors
/// songbird's, when the track has already ended and cannot take the handler.
#[allow(clippy::disallowed_methods)]
pub fn add_track_handler<H: EventHandler + 'static>(
    track: &TrackHandle,
    event: Event,
    handler: H,
) -> TrackResult<()> {
    track.add_event(event, Guarded(handler))
}

/// A songbird [`EventHandler`] whose panics are logged and swallowed instead of
/// taking the call's event task down.
struct Guarded<H>(H);

#[async_trait]
impl<H: EventHandler> EventHandler for Guarded<H> {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match AssertUnwindSafe(self.0.act(ctx)).catch_unwind().await {
            Ok(next) => next,
            Err(panic) => {
                tracing::error!(
                    "songbird event handler {} panicked; the call's event task survives: {}",
                    std::any::type_name::<H>(),
                    panic_message(panic.as_ref())
                );
                None
            },
        }
    }
}

/// The text of a panic, from either payload `panic!` produces.
fn panic_message(panic: &(dyn Any + Send)) -> &str {
    if let Some(message) = panic.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message
    } else {
        "a panic with no message"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Panics;

    #[async_trait]
    impl EventHandler for Panics {
        async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
            panic!("TrackHandle::data generic does not match type set in TrackHandle::set_data")
        }
    }

    struct Cancels;

    #[async_trait]
    impl EventHandler for Cancels {
        async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
            Some(Event::Cancel)
        }
    }

    #[tokio::test]
    async fn a_panicking_handler_answers_none_instead_of_unwinding() {
        assert!(Guarded(Panics)
            .act(&EventContext::Track(&[]))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn a_handler_that_returns_is_passed_straight_through() {
        assert!(matches!(
            Guarded(Cancels).act(&EventContext::Track(&[])).await,
            Some(Event::Cancel)
        ));
    }

    struct Counts(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait]
    impl EventHandler for Counts {
        async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        }
    }

    /// Runs a real, unconnected songbird call with a handler that panics on
    /// every tick beside one that counts ticks, and reports whether the counter
    /// was still counting after the first panics.
    // The unguarded case registers through songbird directly: showing what
    // `add_global_handler` prevents is the point of it.
    #[allow(clippy::disallowed_methods)]
    async fn counter_survives(guard_the_panic: bool) -> bool {
        use ::serenity::all::{GuildId, UserId};
        use std::sync::{atomic::Ordering, Arc};
        use std::time::Duration;

        let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut call = Call::standalone(GuildId::new(1), UserId::new(2));
        let every_tick = || Event::Periodic(Duration::from_millis(20), None);
        if guard_the_panic {
            add_global_handler(&mut call, every_tick(), Panics);
        } else {
            call.add_global_event(every_tick(), Panics);
        }
        add_global_handler(&mut call, every_tick(), Counts(ticks.clone()));

        tokio::time::sleep(Duration::from_millis(300)).await;
        let before = ticks.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = ticks.load(Ordering::SeqCst);
        drop(call);
        after > before
    }

    /// The premise, pinned against songbird itself: one unguarded panic stops
    /// every other handler on the call. If songbird ever isolates handlers,
    /// this fails and the guard can go.
    #[tokio::test]
    async fn an_unguarded_panic_stops_the_calls_other_handlers() {
        assert!(!counter_survives(false).await);
    }

    #[tokio::test]
    async fn a_guarded_panic_leaves_the_calls_other_handlers_running() {
        assert!(counter_survives(true).await);
    }

    #[test]
    fn a_panic_message_is_read_from_either_payload() {
        let literal: Box<dyn Any + Send> = Box::new("literal");
        let formatted: Box<dyn Any + Send> = Box::new(String::from("formatted"));
        let neither: Box<dyn Any + Send> = Box::new(7_u8);

        assert_eq!(panic_message(literal.as_ref()), "literal");
        assert_eq!(panic_message(formatted.as_ref()), "formatted");
        assert_eq!(panic_message(neither.as_ref()), "a panic with no message");
    }
}
