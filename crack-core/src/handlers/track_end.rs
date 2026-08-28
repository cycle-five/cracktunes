use crate::{
    db::PgPoolExtPlayLog,
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    messaging::{
        interface::{create_nav_btns, create_queue_embed, send_now_playing},
        messages::{AUTOPLAY_DISABLED_ERROR, AUTOPLAY_DISABLED_SPOTIFY, SPOTIFY_AUTH_FAILED},
    },
    music::query::NewQueryType,
    sources::spotify::{Spotify, SPOTIFY},
    utils::{
        calculate_num_pages, forget_queue_message, set_track_handle_metadata,
        set_track_handle_requesting_user,
    },
    CrackedResult,
    Data, //, Error,
};
use ::serenity::{
    all::{Cache, GenericChannelId},
    async_trait,
    builder::{CreateMessage, EditMessage},
    http::Http,
    model::id::GuildId,
};
use crack_types::NewAuxMetadata;
use crack_types::QueryType;
use serenity::all::{CacheHttp, UserId};
use songbird::input::AuxMetadata;
use songbird::{tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handler for the end of a track event.
// This needs enough context to be able to send messages to the appropriate
// channels for the music player.
pub struct TrackEndHandler {
    pub guild_id: GuildId,
    pub data: Arc<Data>,
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
}

// use crate::commands::play_utils::queue_track_ready_front;
// use crate::commands::play_utils::ready_query2;
pub struct ModifyQueueHandler {
    pub guild_id: GuildId,
    pub data: Arc<Data>,
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

        tracing::trace!("Forgetting skip votes");
        // FIXME
        match self.data.forget_skip_votes(self.guild_id).await {
            Ok(_) => tracing::trace!("Forgot skip votes"),
            Err(e) => tracing::warn!("Error forgetting skip votes: {}", e),
        };

        // A guilty pleasure game owns playback: its per-track handler advances
        // the rounds, so no autopause, no autoplay filler and no duplicate
        // now-playing embed while it runs.
        if self.data.gp_is_playing(self.guild_id) {
            return None;
        }

        if autopause {
            tracing::trace!("Pausing");
            self.call.lock().await.queue().pause().ok();
        } else {
            tracing::trace!("Not pausing");
        }

        let music_channel = self.data.get_music_channel(self.guild_id).await;

        if !autoplay {
            return None;
        }

        if let EventContext::Track(x) = event_ctx {
            tracing::error!("TrackEvent: {:?}", x);
            let states = get_track_states_union(x);
            //if is_stopped(x) || is_errored(x) {
            if states.errored {
                self.data.set_autoplay(self.guild_id, false).await;
                tracing::warn!("autoplay disabled for {}: track errored", self.guild_id);
                // `channel` is not resolved yet at this point, so this can only
                // speak up when a music channel is configured. Better than the
                // silence this replaced (it was a bare `// FIXME: Send error
                // message`), and it does not justify hoisting the channel lookup
                // above the early returns below it.
                if let Some(c) = music_channel {
                    send_plain(c, self.http.clone(), AUTOPLAY_DISABLED_ERROR).await;
                }
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
                    .map(|c| GenericChannelId::new(c.get()))
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
                // Turning a feature the user switched ON back OFF is not something
                // to do silently. This used to be a `tracing::warn!` and nothing
                // else, so from the channel's point of view the music simply
                // stopped and autoplay was mysteriously off.
                self.data.set_autoplay(self.guild_id, false).await;
                tracing::warn!("autoplay disabled for {}: {}", self.guild_id, e);
                announce_autoplay_off(channel, self.http.clone(), &e).await;
                return None;
            },
        };

        let call = self.call.clone();
        match queue_query(query, call).await {
            Ok(_) => (),
            Err(e) => {
                self.data.set_autoplay(self.guild_id, false).await;
                tracing::warn!("autoplay disabled for {}: {}", self.guild_id, e);
                announce_autoplay_off(channel, self.http.clone(), &e).await;
            },
        }

        let chan_id = channel;

        match send_now_playing(chan_id, self.http.clone(), self.call.clone()).await {
            Ok(_) => tracing::trace!("Sent now playing message"),
            Err(e) => tracing::warn!("Error sending now playing message: {}", e),
        };
        None
    }
}

use songbird::input::Input as SongbirdInput;
/// Queues a query and returns the track handle.
pub async fn queue_query(
    query: QueryType,
    call: Arc<Mutex<Call>>,
) -> Result<TrackHandle, CrackedError> {
    // This is a singleton that holds a reqwest client for the music player.
    let client = crate::http_utils::get_client();
    // This call, this is what does all the work
    // let mut input = query.get_query_source(client.clone());
    // let metadata = input.aux_metadata().await.ok()?;
    // let track = call.as_ref().lock().await.enqueue_input(input).await;
    // add_metadata_to_track(&track, metadata).await;
    let qt = NewQueryType(query);
    let (source, metadata_vec): (SongbirdInput, Vec<NewAuxMetadata>) = qt
        .get_track_source_and_metadata(Some(client.clone()))
        .await?;
    let mut track = call.as_ref().lock().await.enqueue_input(source).await;
    if let Some(metadata) = metadata_vec.first() {
        add_metadata_to_track(&mut track, metadata.clone().into()).await?;
    }
    Ok(track)
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
        let cache_http = (Some(&self.cache), self.http.as_ref());
        update_queue_messages(&cache_http, self.data.clone(), &queue, self.guild_id).await;

        None
    }
}

/// Adds metadata to a track handle with a default requesting user.
pub async fn add_metadata_to_track(
    track: &mut TrackHandle,
    metadata: AuxMetadata,
) -> CrackedResult<()> {
    set_track_handle_metadata(track, metadata).await?;
    set_track_handle_requesting_user(track, UserId::new(1)).await?;
    Ok(())
}

/// This function goes through all the active "queue" messages that are still
/// being updated and updates them with the current.
pub async fn update_queue_messages(
    cache_http: &impl CacheHttp,
    data: Arc<Data>,
    tracks: &[TrackHandle],
    guild_id: GuildId,
) {
    let cache_map = data.guild_cache_map.lock().await.clone();

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
            forget_queue_message(data.clone(), message, guild_id)
                .await
                .ok();
        };
    }
}

/// Send a plain-text line to a channel, best effort.
///
/// Failing to deliver an explanation must never be louder than the thing being
/// explained, so a send error is logged and swallowed.
async fn send_plain(channel: GenericChannelId, http: Arc<Http>, content: &str) {
    if let Err(e) = channel
        .send_message(&http, CreateMessage::new().content(content))
        .await
    {
        tracing::warn!("could not send autoplay notice to {}: {}", channel, e);
    }
}

/// Tell the channel autoplay has been switched off, and say which kind of
/// problem caused it.
///
/// The distinction is the point. "Spotify is unavailable" is a standing
/// condition the listener can route around by queueing tracks themselves;
/// "I couldn't work out what to play next" is a one-off. Collapsing both into a
/// single message trains people to ignore it.
async fn announce_autoplay_off(channel: GenericChannelId, http: Arc<Http>, err: &CrackedError) {
    let content = match err {
        CrackedError::SpotifyAuth
        | CrackedError::RSpotify(_)
        | CrackedError::RSpotifyLockError(_) => AUTOPLAY_DISABLED_SPOTIFY,
        CrackedError::Other(msg) if *msg == SPOTIFY_AUTH_FAILED => AUTOPLAY_DISABLED_SPOTIFY,
        _ => AUTOPLAY_DISABLED_ERROR,
    };
    send_plain(channel, http, content).await;
}

/// Get's the recommended tracks for a guild. Returns `QueryType::None` on failure.
/// Looks at the top
async fn get_recommended_track_query(
    pool: &sqlx::PgPool,
    guild_id: GuildId,
) -> CrackedResult<QueryType> {
    let spotify = SPOTIFY.lock().await;
    let spotify = verify(spotify.as_ref(), CrackedError::SpotifyAuth)?;

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
