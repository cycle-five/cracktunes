//! State-only ops: nothing in Discord shows this state, so they settle `Nothing`.
use super::*;
use crate::guild::operations::GuildSettingsOperations;
use crate::{
    messaging::message::CrackedMessage,
    music::{
        audit::Action,
        queue::{pause_queue, resume_queue},
    },
};
use songbird::tracks::{LoopState, TrackHandle};
use std::time::Duration;
use tokio::time::timeout;

/// A stalled driver must not stall `/volume`.
const TRACK_INFO_TIMEOUT: Duration = Duration::from_secs(1);
/// The track volume shown when the track cannot be asked.
const FALLBACK_TRACK_VOLUME: f32 = 0.1;

#[derive(Debug)]
pub struct Paused;
#[derive(Debug)]
pub struct Resumed;
#[derive(Debug)]
pub struct Sought {
    pub to: Duration,
}
/// Fractions: 0.5 = 50%.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeSet {
    pub old: f32,
    pub new: f32,
}
/// Fractions: 0.5 = 50%.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeNow {
    pub setting: f32,
    pub track: f32,
}
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
impl VolumeSet {
    pub fn description(&self) -> String {
        format!(
            "Volume changed from {:.0}% to {:.0}%",
            self.old * 100.0,
            self.new * 100.0
        )
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

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
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

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
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

pub async fn seek(cx: &OpCx, to: Duration) -> Result<Done<Sought>, OpRefused> {
    let (g, call) = begin(cx).await?;
    seek_on(&g, &call, to).await
}

pub(crate) async fn seek_on(
    _g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    to: Duration,
) -> Result<Done<Sought>, OpRefused> {
    let track = call
        .lock()
        .await
        .queue()
        .current()
        .ok_or(OpRefused::NothingPlaying)?;
    track
        .seek(to)
        .result_async()
        .await
        .map_err(OpRefused::SeekFailed)?;
    Ok(Done {
        outcome: Sought { to },
        settle: Settle::Nothing,
        call: Some(call.clone()),
    })
}

/// Set the volume to `percent`% in settings and on the playing track.
pub async fn volume(cx: &OpCx, percent: u32) -> Result<Done<VolumeSet>, OpRefused> {
    let (g, call) = begin(cx).await?;
    volume_on(&g, &call, &cx.data, percent).await
}

pub(crate) async fn volume_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    data: &Data,
    percent: u32,
) -> Result<Done<VolumeSet>, OpRefused> {
    let guild = g.guild_id();
    let new = percent as f32 / 100.0;
    let old = data.set_volume(guild, new).await;
    let current = call.lock().await.queue().current();
    if let Some(track) = current {
        if let Err(e) = track.set_volume(new) {
            tracing::warn!("could not set the track volume: {e}");
        }
    }
    Ok(Done {
        outcome: VolumeSet { old, new },
        settle: Settle::Nothing,
        call: Some(call.clone()),
    })
}

/// A read: no lease, nothing to settle.
pub async fn volume_now(cx: &OpCx) -> Result<VolumeNow, OpRefused> {
    let call = connected_call(&cx.data.songbird, cx.guild_id, None)
        .await
        .ok_or(OpRefused::NotConnected)?;
    let setting = cx.data.get_volume(cx.guild_id).await.0;
    let current = call.lock().await.queue().current();
    let track = match current {
        Some(track) => match timeout(TRACK_INFO_TIMEOUT, track.get_info()).await {
            Ok(Ok(info)) => info.volume,
            Ok(Err(e)) => {
                tracing::warn!("could not read the track volume: {e}");
                FALLBACK_TRACK_VOLUME
            },
            Err(_) => {
                tracing::warn!("reading the track volume timed out");
                FALLBACK_TRACK_VOLUME
            },
        },
        None => FALLBACK_TRACK_VOLUME,
    };
    Ok(VolumeNow { setting, track })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::{audit::Action, ops::test_support::*};
    use std::time::Duration;

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

    #[tokio::test]
    async fn volume_reports_the_real_old_value() {
        use crate::guild::{operations::GuildSettingsOperations, settings::GuildSettings};
        let (data, _, _, _) = queue_of(0).await;
        data.guild_settings_map
            .write()
            .await
            .insert(GUILD, GuildSettings::default());
        data.set_volume(GUILD, 0.3).await;
        let old = data.set_volume(GUILD, 0.7).await;
        assert_eq!(old, 0.3);
        assert_eq!(data.get_volume(GUILD).await.0, 0.7);
    }

    #[tokio::test]
    async fn setting_volume_never_creates_settings() {
        use crate::guild::{operations::GuildSettingsOperations, settings::DEFAULT_VOLUME_LEVEL};
        let (data, _, _, _) = queue_of(0).await;
        assert_eq!(data.set_volume(GUILD, 0.7).await, DEFAULT_VOLUME_LEVEL);
        assert!(data.guild_settings_map.read().await.get(&GUILD).is_none());
    }

    #[tokio::test]
    async fn volume_op_returns_old_and_new_as_fractions() {
        use crate::guild::{operations::GuildSettingsOperations, settings::GuildSettings};
        let (data, call, _, mut rx) = queue_of(1).await;
        data.guild_settings_map
            .write()
            .await
            .insert(GUILD, GuildSettings::default());
        data.set_volume(GUILD, 0.3).await;
        let g = guard(&data).await;
        let done = volume_on(&g, &call, &data, 70).await.unwrap();
        assert_eq!(done.settle, Settle::Nothing);
        assert_eq!(done.outcome, VolumeSet { old: 0.3, new: 0.7 });
        assert_eq!(data.get_volume(GUILD).await.0, 0.7);
        assert!(recorded(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn seeking_nothing_is_refused() {
        let (data, call, _, _) = queue_of(0).await;
        let g = guard(&data).await;
        assert!(matches!(
            seek_on(&g, &call, Duration::from_secs(5)).await,
            Err(OpRefused::NothingPlaying)
        ));
    }

    #[tokio::test]
    async fn volume_and_seek_with_no_call_are_not_connected() {
        let cx = cx_without_call();
        assert!(matches!(
            volume(&cx, 50).await,
            Err(OpRefused::NotConnected)
        ));
        assert!(matches!(
            seek(&cx, Duration::from_secs(1)).await,
            Err(OpRefused::NotConnected)
        ));
        assert!(matches!(
            volume_now(&cx).await,
            Err(OpRefused::NotConnected)
        ));
    }

    #[test]
    fn the_volume_reply_names_old_and_new() {
        assert_eq!(
            VolumeSet { old: 0.3, new: 0.7 }.description(),
            "Volume changed from 30% to 70%"
        );
    }
}
