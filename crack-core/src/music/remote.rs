//! Queue operations for callers with no poise `Context` -- the web dashboard.
//!
//! Everything that touches songbird for the dashboard lives here, inside
//! crack-core, so it stays behind this crate's `clippy.toml` bans (no
//! `Songbird::get`, no `TrackHandle::data`). crack-web only sees the plain
//! types below.

use crate::{
    commands::music_utils::connected_call,
    handlers::track_end::update_queue_messages,
    music::{move_track_by_id, PlaybackOwner},
    utils::{get_requesting_user, get_track_handle_metadata},
    CrackedError, Data,
};
use serenity::all::{ChannelId, GuildId, Http, UserId};
use songbird::{tracks::TrackHandle, Call};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Who queued a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requester {
    /// Autoplay picked it (stored as user id 1, see `requesting_user_to_string`).
    Auto,
    User(UserId),
}

/// One queued track, as the dashboard shows it.
#[derive(Debug, Clone)]
pub struct TrackSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
    pub requester: Option<Requester>,
}

/// A guild's queue, as the dashboard may show it.
#[derive(Debug, Clone)]
pub enum QueueState {
    /// Not connected, or nothing queued.
    Idle,
    /// A `/gp` game owns playback: the queue would give the answers away.
    Hidden,
    /// `tracks[0]` is playing; `bot_channel` is where.
    Playing {
        bot_channel: ChannelId,
        tracks: Vec<TrackSummary>,
    },
}

/// The queue in `guild_id`. A game hides it before the call is even looked up.
pub async fn queue_state(data: &Data, guild_id: GuildId) -> QueueState {
    unless_a_game(data, guild_id, async {
        match connected_call(&data.songbird, guild_id, None).await {
            Some(call) => state_of_call(&call).await,
            None => QueueState::Idle,
        }
    })
    .await
}

/// `read`, unless a game owns playback before it starts or by the time it
/// ends: a game that claims and enqueues mid-read must not have its first
/// title published.
async fn unless_a_game(
    data: &Data,
    guild_id: GuildId,
    read: impl Future<Output = QueueState>,
) -> QueueState {
    if data.playback_owner(guild_id) != PlaybackOwner::Free {
        return QueueState::Hidden;
    }
    let state = read.await;
    if data.playback_owner(guild_id) != PlaybackOwner::Free {
        return QueueState::Hidden;
    }
    state
}

/// The queue on one call. The call lock is held only to clone the handles;
/// metadata is read after it is released.
pub(crate) async fn state_of_call(call: &Arc<Mutex<Call>>) -> QueueState {
    let (channel, handles) = {
        let handler = call.lock().await;
        (handler.current_channel(), handler.queue().current_queue())
    };
    match channel {
        Some(channel) if !handles.is_empty() => QueueState::Playing {
            bot_channel: ChannelId::new(channel.get()),
            tracks: summarize(&handles).await,
        },
        _ => QueueState::Idle,
    }
}

/// Read what the dashboard shows from each handle.
pub(crate) async fn summarize(handles: &[TrackHandle]) -> Vec<TrackSummary> {
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        // No metadata is a blank row, not an error: `/queue` does the same.
        let meta = get_track_handle_metadata(handle).await.unwrap_or_default();
        let requester = get_requesting_user(handle).await.ok().map(|u| {
            if u.get() == 1 {
                Requester::Auto
            } else {
                Requester::User(u)
            }
        });
        out.push(TrackSummary {
            id: handle.uuid(),
            title: meta.title,
            url: meta.source_url,
            duration: meta.duration,
            requester,
        });
    }
    out
}

/// The voice channel the bot is connected to in `guild_id`, if any.
pub async fn bot_channel(data: &Data, guild_id: GuildId) -> Option<ChannelId> {
    let call = connected_call(&data.songbird, guild_id, None).await?;
    let channel = call.lock().await.current_channel()?;
    Some(ChannelId::new(channel.get()))
}

/// Every guild with a connected call, and the channel it is in.
pub async fn active_guilds(data: &Data) -> Vec<(GuildId, ChannelId)> {
    let calls: Vec<_> = data.songbird.iter().collect();
    let mut out = Vec::new();
    for (guild_id, call) in calls {
        let handler = call.lock().await;
        if handler.current_connection().is_none() {
            continue;
        }
        if let Some(channel) = handler.current_channel() {
            out.push((GuildId::new(guild_id.get()), ChannelId::new(channel.get())));
        }
    }
    out
}

/// Why a move was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefused {
    /// The bot is not connected in this guild.
    NotPlaying,
    /// A `/gp` game owns playback.
    GameInProgress,
    /// No track with that id is queued (it finished, or was removed).
    Absent,
    /// That track is the one playing; only upcoming tracks move.
    NowPlaying,
}

/// Move a track by id. The queue messages in Discord are refreshed in the
/// background: they are edited one by one under Discord's per-channel rate
/// limit, and the move is done whether or not they have caught up. Posts no
/// reply: a drag is silent in the channel (owner's decision).
pub async fn move_by_id(
    data: Arc<Data>,
    http: Arc<Http>,
    guild_id: GuildId,
    id: Uuid,
    to_upcoming: usize,
) -> Result<usize, MoveRefused> {
    // The lease first: a game refuses at once, before the call is touched.
    let guard = data
        .lock_queue(guild_id, PlaybackOwner::Free)
        .await
        .map_err(|e| match e {
            CrackedError::GameInProgress => MoveRefused::GameInProgress,
            other => {
                tracing::warn!("lock_queue refused a dashboard move: {other}");
                MoveRefused::GameInProgress
            },
        })?;
    let call = connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(MoveRefused::NotPlaying)?;
    let handler = call.lock().await;
    let moved = move_track_by_id(&guard, &handler, id, to_upcoming);
    // Held only for the mutation, as every command does -- see lease.rs.
    drop(guard);
    let queue = handler.queue().current_queue();
    drop(handler);
    if moved.is_ok() {
        tokio::spawn(async move {
            update_queue_messages(&*http, data, &queue, guild_id).await;
        });
    }
    moved
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Data, DataInner};
    use serenity::all::{GuildId, Http};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    fn data() -> Data {
        Data(Arc::new(DataInner::default()))
    }

    #[tokio::test]
    async fn a_game_hides_the_queue_before_anything_else_is_asked() {
        let d = data();
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        assert!(matches!(queue_state(&d, G).await, QueueState::Hidden));
    }

    #[tokio::test]
    async fn a_game_is_checked_before_the_read_starts() {
        let d = data();
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        let got = unless_a_game(&d, G, async { panic!("the call was read") }).await;
        assert!(matches!(got, QueueState::Hidden));
    }

    #[tokio::test]
    async fn a_game_that_starts_during_the_read_hides_what_it_read() {
        let d = data();
        let got = unless_a_game(&d, G, async {
            // A game claims playback and enqueues while the call is summarized.
            d.claim_playback(G, crate::music::PlaybackOwner::Game)
                .unwrap();
            QueueState::Playing {
                bot_channel: ChannelId::new(2),
                tracks: vec![],
            }
        })
        .await;
        assert!(matches!(got, QueueState::Hidden));
    }

    #[tokio::test]
    async fn no_call_is_idle() {
        assert!(matches!(queue_state(&data(), G).await, QueueState::Idle));
        assert_eq!(bot_channel(&data(), G).await, None);
        assert!(active_guilds(&data()).await.is_empty());
    }

    #[tokio::test]
    async fn a_game_refuses_a_move_before_the_call_is_looked_up() {
        let d = Arc::new(data());
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = move_by_id(d, http, G, uuid::Uuid::from_u128(1), 0).await;
        assert_eq!(got, Err(MoveRefused::GameInProgress));
    }

    #[tokio::test]
    async fn a_move_with_no_call_is_not_playing() {
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = move_by_id(Arc::new(data()), http, G, uuid::Uuid::from_u128(1), 0).await;
        assert_eq!(got, Err(MoveRefused::NotPlaying));
    }
}
