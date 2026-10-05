//! Ops that end playback: stop everything, or leave voice.
use super::*;
use crate::{
    guild::operations::GuildSettingsOperations,
    messaging::message::CrackedMessage,
    music::{disconnect::disconnect, queue::stop_queue},
};
use songbird::error::JoinError;

#[derive(Debug)]
pub struct Stopped {
    pub removed: usize,
}
#[derive(Debug)]
pub struct Left;

impl Stopped {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Stop
    }
}
impl Left {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Leaving
    }
}

/// Turns autoplay off, then stops everything. Settles `Finished`.
pub async fn stop(cx: &OpCx) -> Result<Done<Stopped>, OpRefused> {
    // As /stop always did: autoplay off first, so the emptied queue is not refilled.
    cx.data.set_autoplay(cx.guild_id, false).await;
    let (g, call) = begin(cx).await?;
    stop_on(&g, &call).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn stop_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
) -> Result<Done<Stopped>, OpRefused> {
    let handler = call.lock().await;
    let removed = handler.queue().len();
    if removed == 0 {
        return Err(OpRefused::NothingPlaying);
    }
    stop_queue(g, &handler);
    Ok(Done {
        outcome: Stopped { removed },
        settle: Settle::Finished,
        call: Some(call.clone()),
    })
}

/// Leaves voice. Takes no lease, as `/leave` never did: it must work during a
/// `/gp` game. `disconnect` records the discard itself. Settles `Finished`.
pub async fn leave(cx: &OpCx) -> Result<Done<Left>, OpRefused> {
    match disconnect(&cx.data, &cx.data.songbird, cx.guild_id, cx.actor.clone()).await {
        Ok(()) => {
            tracing::info!("Driver successfully removed.");
            Ok(Done {
                outcome: Left,
                settle: Settle::Finished,
                call: None,
            })
        },
        Err(JoinError::NoCall) => Err(OpRefused::NotConnected),
        Err(err) => {
            tracing::error!("Driver could not be removed: {}", err);
            Err(OpRefused::LeaveFailed(err))
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::{audit::Action, ops::test_support::*};

    #[tokio::test]
    async fn stop_empties_the_queue_and_settles_finished() {
        let (data, call, _, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        let d = stop_on(&g, &call).await.unwrap();
        assert_eq!((d.outcome.removed, d.settle), (3, Settle::Finished));
        assert!(ids_of(&call).await.is_empty());
        assert!(matches!(
            recorded(&mut rx).as_slice(),
            [Action::Stop { removed: 3 }]
        ));
    }

    #[tokio::test]
    async fn stopping_nothing_is_nothing_playing() {
        let (data, call, _, _) = queue_of(0).await;
        let g = guard(&data).await;
        assert!(matches!(
            stop_on(&g, &call).await,
            Err(OpRefused::NothingPlaying)
        ));
    }

    #[tokio::test]
    async fn leaving_with_no_call_is_not_connected() {
        let cx = cx_without_call();
        assert!(matches!(leave(&cx).await, Err(OpRefused::NotConnected)));
    }

    #[tokio::test]
    async fn stop_with_no_call_turns_autoplay_off_and_is_not_connected() {
        let cx = cx_without_call();
        cx.data.set_autoplay(cx.guild_id, true).await;
        assert!(matches!(stop(&cx).await, Err(OpRefused::NotConnected)));
        assert!(!cx.data.get_autoplay(cx.guild_id).await);
    }

    #[tokio::test]
    async fn leaving_during_a_game_takes_no_lease() {
        let cx = cx_without_call();
        cx.data
            .claim_playback(cx.guild_id, crate::music::PlaybackOwner::Game)
            .unwrap();
        // A lease would refuse GameInProgress; leave must reach the manager.
        assert!(matches!(leave(&cx).await, Err(OpRefused::NotConnected)));
    }

    #[test]
    fn outcomes_say_what_the_commands_always_said() {
        assert_eq!(Stopped { removed: 1 }.message(), CrackedMessage::Stop);
        assert_eq!(Left.message(), CrackedMessage::Leaving);
    }

    #[test]
    fn a_failed_leave_maps_to_the_join_error() {
        let want = CrackedError::from(JoinError::TimedOut).to_string();
        let got = CrackedError::from(OpRefused::LeaveFailed(JoinError::TimedOut)).to_string();
        assert_eq!(got, want);
    }
}
