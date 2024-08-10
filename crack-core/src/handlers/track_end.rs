use crate::{
    commands::{forget_skip_votes, play_utils::QueryType},
    db::PgPoolExtPlayLog,
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    messaging::{
        interface::{create_nav_btns, create_queue_embed, send_now_playing},
        messages::SPOTIFY_AUTH_FAILED,
    },
    sources::spotify::{Spotify, SPOTIFY},
    utils::{calculate_num_pages, forget_queue_message},
    CrackedResult,
    Data, //, Error,
};
use ::serenity::{
    all::{Cache, ChannelId},
    async_trait,
    builder::EditMessage,
    http::Http,
    model::id::GuildId,
};
use serenity::all::CacheHttp;
use songbird::{tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handler for the end of a track event.
// This needs enough context to be able to send messages to the appropriate
// channels for the music player.
pub struct TrackEndHandler {
    pub guild_id: GuildId,
    pub data: Data,
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
}

use crate::commands::play_utils::queue_track_ready_front;
use crate::commands::play_utils::ready_query2;
pub struct ModifyQueueHandler {
    pub guild_id: GuildId,
    pub data: Data,
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub call: Arc<Mutex<Call>>,
}

use songbird::tracks::PlayMode;
use songbird::tracks::TrackState;
type TrackStates<'a> = &'a [(&'a TrackState, &'a TrackHandle)];

pub struct TrackStatesUnion {
    pub playing: bool,
    pub paused: bool,
    pub stopped: bool,
    pub errored: bool,
    pub end: bool,
}

fn get_track_states_union(track_states: TrackStates) -> TrackStatesUnion {
    let mut union = TrackStatesUnion {
        playing: false,
        paused: false,
        stopped: false,
        errored: false,
        end: false,
    };

    for (state, _) in track_states.iter() {
        match state.playing {
            PlayMode::Play => union.playing = true,
            PlayMode::Pause => union.paused = true,
            PlayMode::Stop => union.stopped = true,
            PlayMode::End => union.end = true,
            PlayMode::Errored(_) => union.errored = true,
            _ => (),
        }
    }

    union
}

// fn is_playing(track_states: TrackStates) -> bool {
//     track_states
//         .iter()
//         .any(|(state, _)| state.playing == PlayMode::Play)
// }

// fn is_paused(track_states: TrackStates) -> bool {
//     track_states
//         .iter()
//         .any(|(state, _)| state.playing == PlayMode::Pause)
// }

// fn is_stopped(track_states: TrackStates) -> bool {
//     track_states
//         .iter()
//         .any(|(state, _)| state.playing == PlayMode::Stop)
// }

// fn is_end(track_states: TrackStates) -> bool {
//     track_states
//         .iter()
//         .any(|(state, _)| state.playing == PlayMode::End)
// }

// fn is_errored(track_states: TrackStates) -> bool {
//     track_states
//         .iter()
//         .any(|(state, _)| matches!(state.playing, PlayMode::Errored(_)))
// }

/// Event handler to handle the end of a track.
#[async_trait]
impl EventHandler for TrackEndHandler {
    async fn act(&self, event_ctx: &EventContext<'_>) -> Option<Event> {
        tracing::error!("TrackEndHandler");
        // Handle track error

        let autoplay = self.data.get_autoplay(self.guild_id).await;

        tracing::error!("Autoplay: {}", autoplay);

        let (autopause, _volume) = {
            let settings = self.data.guild_settings_map.read().await.clone();
            let autopause = settings
                .get(&self.guild_id)
                .map(|guild_settings| guild_settings.autopause)
                .unwrap_or_default();
            tracing::error!("Autopause: {}", autopause);
            let volume = settings
                .get(&self.guild_id)
                .map(|guild_settings| guild_settings.volume)
                .unwrap_or(crate::guild::settings::DEFAULT_VOLUME_LEVEL);
            tracing::error!("Volume: {}", volume);
            (autopause, volume)
        };

        if autopause {
            tracing::trace!("Pausing");
            self.call.lock().await.queue().pause().ok();
        } else {
            tracing::trace!("Not pausing");
        }

        tracing::trace!("Forgetting skip votes");
        // FIXME
        match forget_skip_votes(&self.data, self.guild_id).await {
            Ok(_) => tracing::trace!("Forgot skip votes"),
            Err(e) => tracing::warn!("Error forgetting skip votes: {}", e),
        };

        let music_channel = self.data.get_music_channel(self.guild_id).await;

        if !autoplay {
            return None;
        }

        if let EventContext::Track(x) = event_ctx {
            tracing::error!("TrackEvent: {:?}", x);
            let states = get_track_states_union(x);
            //if is_stopped(x) || is_errored(x) {
            if states.stopped || states.errored {
                self.data.set_autoplay(self.guild_id, false).await;
                return None;
            }
        }

        let pool = if let Some(pool) = &self.data.database_pool {
            pool
        } else {
            return None;
        };

        let (channel, next_track) = {
            let handler = self.call.lock().await;
            let channel = match music_channel {
                Some(c) => c,
                _ => handler
                    .current_channel()
                    .map(|c| ChannelId::new(c.0.get()))
                    .unwrap(),
            };
            let track = handler.queue().current().clone();
            (channel, track)
        };

        if next_track.is_some() {
            send_now_playing(channel, self.http.clone(), self.call.clone())
                .await
                .ok();
            return None;
        }

        let query = match get_recommended_track_query(pool, self.guild_id).await {
            Ok(query) => query,
            Err(e) => {
                self.data.set_autoplay(self.guild_id, false).await;
                let msg = format!("Error: {}", e);
                tracing::warn!("{}", msg);
                return None;
            },
        };
        let track_ready = match ready_query2(query).await {
            Ok(track) => track,
            Err(e) => {
                self.data.set_autoplay(self.guild_id, false).await;
                let msg = format!("Error: {}", e);
                tracing::warn!("{}", msg);
                return None;
            },
        };
        // let MyAuxMetadata(metadata) = &track_ready.metadata;
        // let metadata = Some(metadata.clone());

        let _track = queue_track_ready_front(&self.call, track_ready)
            .await
            .ok()?;

        let chan_id = channel;

        match send_now_playing(chan_id, self.http.clone(), self.call.clone()).await {
            Ok(_) => tracing::trace!("Sent now playing message"),
            Err(e) => tracing::warn!("Error sending now playing message: {}", e),
        };
        None
    }
}

/// Event handler to set the volume of the playing track to the volume
/// set in the guild settings after a queue modification.
#[async_trait]
impl EventHandler for ModifyQueueHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let queue = {
            let handler = self.call.lock().await;
            handler.queue().current_queue()
        };
        let vol = {
            let guild_settings = self.data.get_guild_settings(self.guild_id).await;
            guild_settings.map(|x| x.volume)
        };

        vol.map(|vol| queue.first().map(|track| track.set_volume(vol).unwrap()));
        let cache_http = (&self.cache, self.http.as_ref());
        update_queue_messages(&cache_http, &self.data, &queue, self.guild_id).await;

        None
    }
}

/// This function goes through all the active "queue" messages that are still
/// being updated and updates them with the current.
pub async fn update_queue_messages(
    cache_http: &impl CacheHttp,
    data: &Data,
    tracks: &[TrackHandle],
    guild_id: GuildId,
) {
    let cache_map = data.guild_cache_map.lock().await;

    let mut messages = match cache_map.get(&guild_id) {
        Some(cache) => cache.queue_messages.clone(),
        None => return,
    };

    for (message, page_lock) in messages.iter_mut() {
        // has the page size shrunk?
        let num_pages = calculate_num_pages(tracks);
        let page = *page_lock.read().await;
        let page_val = usize::min(page, num_pages - 1);
        *page_lock.write().await = page_val;

        let embed = create_queue_embed(tracks, page_val).await;

        let edit_message = message
            .edit(
                cache_http,
                EditMessage::new()
                    .embed(embed)
                    .components(create_nav_btns(page_val, num_pages)),
            )
            .await;

        if edit_message.is_err() {
            forget_queue_message(data, message, guild_id).await.ok();
        };
    }
}

/// Get's the recommended tracks for a guild. Returns `QueryType::None` on failure.
/// Looks at the top
async fn get_recommended_track_query(
    pool: &sqlx::PgPool,
    guild_id: GuildId,
) -> CrackedResult<QueryType> {
    let spotify = SPOTIFY.lock().await;
    let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;

    let last_played = pool.get_last_played_by_guild(guild_id, 5).await?;
    let res_rec = Spotify::get_recommendations(spotify, last_played.clone()).await?;

    if res_rec.is_empty() {
        return Ok(QueryType::None);
    }

    match Spotify::search(spotify, &res_rec[0]).await {
        Ok(query) => Ok(query),
        Err(e) => Err(e),
    }
}
