//! State-only ops: nothing in Discord shows this state, so they settle `Nothing`.
use super::*;
use crate::{
    messaging::message::CrackedMessage,
    music::{
        audit::Action,
        queue::{pause_queue, resume_queue},
    },
};
use songbird::tracks::{LoopState, TrackHandle};

#[derive(Debug)]
pub struct Paused;
#[derive(Debug)]
pub struct Resumed;
#[derive(Debug)]
pub struct Repeat {
    pub on: bool,
}

impl Paused {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Pause
    }
}
impl Resumed {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Resume
    }
}
impl Repeat {
    pub fn message(&self) -> CrackedMessage {
        if self.on {
            CrackedMessage::LoopEnable
        } else {
            CrackedMessage::LoopDisable
        }
    }
}

pub async fn pause(cx: &OpCx) -> Result<Done<Paused>, OpRefused> {
    let (g, call) = begin(cx).await?;
    pause_on(&g, &call).await
}

pub(crate) async fn pause_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
) -> Result<Done<Paused>, OpRefused> {
    let handler = call.lock().await;
    if handler.queue().is_empty() {
        return Err(OpRefused::NothingPlaying);
    }
    pause_queue(g, &handler).map_err(|_| OpRefused::Failed(Failure::Pause))?;
    Ok(Done {
        outcome: Paused,
        settle: Settle::Nothing,
        call: Some(call.clone()),
    })
}

pub async fn resume(cx: &OpCx) -> Result<Done<Resumed>, OpRefused> {
    let (g, call) = begin(cx).await?;
    resume_on(&g, &call).await
}

pub(crate) async fn resume_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
) -> Result<Done<Resumed>, OpRefused> {
    let handler = call.lock().await;
    if handler.queue().is_empty() {
        return Err(OpRefused::NothingPlaying);
    }
    resume_queue(g, &handler).map_err(|_| OpRefused::Failed(Failure::Resume))?;
    Ok(Done {
        outcome: Resumed,
        settle: Settle::Nothing,
        call: Some(call.clone()),
    })
}

/// `None` toggles; `Some(on)` sets.
pub async fn repeat(cx: &OpCx, on: Option<bool>) -> Result<Done<Repeat>, OpRefused> {
    let (g, call) = begin(cx).await?;
    repeat_on(&g, &call, on).await
}

pub(crate) async fn repeat_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    on: Option<bool>,
) -> Result<Done<Repeat>, OpRefused> {
    let (track, voice) = {
        let handler = call.lock().await;
        (
            handler.queue().current().ok_or(OpRefused::NothingPlaying)?,
            handler.current_channel(),
        )
    };
    let on = match on {
        Some(on) => on,
        None => {
            let info = track
                .get_info()
                .await
                .map_err(|_| OpRefused::Failed(Failure::Loop))?;
            info.loops != LoopState::Infinite
        },
    };
    let set = if on {
        TrackHandle::enable_loop(&track)
    } else {
        TrackHandle::disable_loop(&track)
    };
    set.map_err(|_| OpRefused::Failed(Failure::Loop))?;
    g.record(voice, Action::Repeat { on });
    Ok(Done {
        outcome: Repeat { on },
        settle: Settle::Nothing,
        call: Some(call.clone()),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::{audit::Action, ops::test_support::*};

    #[tokio::test]
    async fn pause_and_resume_record_and_settle_nothing() {
        let (data, call, _, mut rx) = queue_of(2).await;
        let g = guard(&data).await;
        let done = pause_on(&g, &call).await.unwrap();
        assert_eq!(done.settle, Settle::Nothing);
        let done = resume_on(&g, &call).await.unwrap();
        assert_eq!(done.settle, Settle::Nothing);
        assert_eq!(recorded(&mut rx), vec![Action::Pause, Action::Resume]);
    }

    #[tokio::test]
    async fn pausing_an_empty_queue_is_nothing_playing() {
        let (data, call, _, mut rx) = queue_of(0).await;
        let g = guard(&data).await;
        assert!(matches!(
            pause_on(&g, &call).await,
            Err(OpRefused::NothingPlaying)
        ));
        assert!(matches!(
            resume_on(&g, &call).await,
            Err(OpRefused::NothingPlaying)
        ));
        assert!(recorded(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn repeat_sets_explicitly_and_records_what_it_set() {
        let (data, call, _, mut rx) = queue_of(1).await;
        let g = guard(&data).await;
        assert!(repeat_on(&g, &call, Some(true)).await.unwrap().outcome.on);
        assert!(!repeat_on(&g, &call, Some(false)).await.unwrap().outcome.on);
        assert_eq!(
            recorded(&mut rx),
            vec![Action::Repeat { on: true }, Action::Repeat { on: false }]
        );
    }

    #[tokio::test]
    async fn repeat_with_nothing_playing_is_refused() {
        let (data, call, _, _) = queue_of(0).await;
        let g = guard(&data).await;
        assert!(matches!(
            repeat_on(&g, &call, Some(true)).await,
            Err(OpRefused::NothingPlaying)
        ));
    }
}
