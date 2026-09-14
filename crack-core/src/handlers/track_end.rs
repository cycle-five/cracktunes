use crate::{
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    messaging::{
        interface::{create_nav_btns, create_queue_embed, send_now_playing},
        messages::{AUTOPLAY_NEEDS_MUSICRECO, AUTOPLAY_STOPPED},
    },
    music::autoplay,
    music::query::NewQueryType,
    music::queue::{enqueue_input_back, pause_queue, preload_from_metadata, track_data},
    music::PlaybackOwner,
    utils::{calculate_num_pages, forget_queue_message, get_track_handle_metadata},
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
use serenity::all::CacheHttp;
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
        // 🪤 These five were ERROR (#512) -- leftover printf debugging, not
        // failures. The rest of this function already uses `trace!` for its
        // progress and `warn!` for real problems, which is what made them
        // stand out. At five lines per track ending per guild, with a `/gp`
        // game ending tracks in a loop, they were most of what a genuine
        // ERROR had to be found among.
        tracing::trace!("TrackEndHandler");
        // Handle track error

        let autoplay = self.data.get_autoplay(self.guild_id).await;

        tracing::trace!("Autoplay: {}", autoplay);

        let (autopause, _volume) = {
            let settings = self.data.guild_settings_map.read().await.clone();
            let autopause = settings
                .get(&self.guild_id)
                .map(|guild_settings| guild_settings.autopause)
                .unwrap_or_default();
            tracing::trace!("Autopause: {}", autopause);
            let volume = settings
                .get(&self.guild_id)
                .map(|guild_settings| guild_settings.volume)
                .unwrap_or(crate::guild::settings::DEFAULT_VOLUME_LEVEL);
            tracing::trace!("Volume: {}", volume);
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
        if self.data.gp_is_active(self.guild_id) {
            // A game parked by `/gp end` stayed in the map for exactly this event:
            // `stop()` queues the `End` rather than firing it, so the game has to
            // outlive the command or this handler treats the result as an ordinary
            // track ending and starts autoplay. That `End` is here, so collect it.
            if self.data.gp_remove_if_parked(self.guild_id) {
                tracing::trace!("gp: collected the parked game in {}", self.guild_id);
            }
            return None;
        }

        if autopause {
            tracing::trace!("Pausing");
            // Autopause has no command and no user behind it, but it mutates
            // the queue, so it takes the guard like every other mutation.
            // `Free` is correct: the early return above has already left for
            // any guild a game owns, so by the time control reaches this line
            // nothing holds the lease. Passing `Game` here would be refused for
            // every ordinary guild -- which is to say, always.
            match self
                .data
                .lock_queue(self.guild_id, PlaybackOwner::Free)
                .await
            {
                Ok(guard) => {
                    let handler = self.call.lock().await;
                    // Nobody asked for this pause, so a failure has nobody to
                    // report it to.
                    pause_queue(&guard, &handler).ok();
                },
                // A nicety with nobody to answer to: log it and carry on.
                Err(e) => tracing::trace!("autopause skipped in {}: {e}", self.guild_id),
            }
        } else {
            tracing::trace!("Not pausing");
        }

        let music_channel = self.data.get_music_channel(self.guild_id).await;

        if !autoplay {
            return None;
        }

        if let EventContext::Track(x) = event_ctx {
            // `debug!` rather than `trace!`: this one is genuinely useful when
            // debugging playback, being the whole track-state slice. It is
            // still not an error.
            tracing::debug!("TrackEvent: {:?}", x);
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
                    send_plain(c, self.http.clone(), AUTOPLAY_STOPPED).await;
                }
                return None;
            }
        }

        // The track that just ended seeds the next recommendation. No database:
        // neither YouTube's Mix nor Deezer needs one, so autoplay works without one.
        let ended: Option<TrackHandle> = match event_ctx {
            EventContext::Track(tracks) => tracks.first().map(|(_, handle)| (*handle).clone()),
            _ => None,
        };

        let (channel, next_track) = {
            let handler = self.call.lock().await;
            let fallback = handler
                .current_channel()
                .map(|c| GenericChannelId::new(c.get()));
            let Some(channel) = music_channel.or(fallback) else {
                // Not connected any more: nowhere to announce, nothing to play
                // into. This was an `unwrap` on a tokio worker.
                return None;
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

        // 🔴 This replaces a Spotify path that could never run: it needed client
        // credentials production does not have, and Spotify stopped issuing new
        // Web API apps around 2025-12.
        let Some(next) = self.next_autoplay_track(ended).await else {
            // Turning a feature the user switched ON back OFF is not something
            // to do silently: from the channel's point of view the music would
            // simply stop.
            self.data.set_autoplay(self.guild_id, false).await;
            tracing::warn!("autoplay disabled for {}: no recommendation", self.guild_id);
            announce_autoplay_off(channel, self.http.clone(), self.data.musicreco.is_some()).await;
            return None;
        };
        tracing::debug!(
            "autoplay in {}: `{} - {}` from {}",
            self.guild_id,
            next.artist,
            next.title,
            next.source
        );
        let query = autoplay::to_query(&next);

        let call = self.call.clone();
        match queue_query(&self.data, self.guild_id, query, call).await {
            Ok(_) => (),
            Err(e) => {
                self.data.set_autoplay(self.guild_id, false).await;
                tracing::warn!("autoplay disabled for {}: {}", self.guild_id, e);
                announce_autoplay_off(channel, self.http.clone(), self.data.musicreco.is_some())
                    .await;
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

impl TrackEndHandler {
    /// The guild's next recommendation: the front of its buffer, or a refill
    /// seeded from the track that just ended.
    async fn next_autoplay_track(
        &self,
        ended: Option<TrackHandle>,
    ) -> Option<crack_musicreco::Recommendation> {
        let reco = self.data.musicreco.clone()?;
        let guild_id = self.guild_id;
        self.data
            .autoplay_buffer
            .next(guild_id, || async move {
                let Some(ended) = ended else {
                    return Vec::new();
                };
                let meta = match get_track_handle_metadata(&ended).await {
                    Ok(meta) => meta,
                    Err(e) => {
                        tracing::debug!(
                            "autoplay in {guild_id}: no metadata on the ended track: {e}"
                        );
                        return Vec::new();
                    },
                };
                let Some(raw) = autoplay::raw_track(&meta) else {
                    tracing::debug!("autoplay in {guild_id}: the ended track has no title");
                    return Vec::new();
                };
                reco.next_tracks(&raw, autoplay::REFILL_SIZE)
                    .await
                    .unwrap_or_default()
            })
            .await
    }
}

use songbird::input::Input as SongbirdInput;
/// Queues a query and returns the track handle.
///
/// Resolves first, *then* acquires the [`QueueGuard`], the same shape as
/// [`queue_track_back`](crate::music::queue::queue_track_back): resolution is
/// the slow leg (8-15s cold) and `lease.rs` forbids holding exclusion across
/// one, so the guard cannot be supplied by the caller.
///
/// [`PlaybackOwner::Free`] is what it locks as. Its only caller is the autoplay
/// tail of [`TrackEndHandler::act`], which has already returned early for any
/// guild a game owns, so nothing holds the lease by then -- and locking as
/// `Game` would be refused for every ordinary guild.
pub async fn queue_query(
    data: &Data,
    guild_id: GuildId,
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
    enqueue_resolved_autoplay(data, guild_id, &call, source, metadata_vec).await
}

/// The half of [`queue_query`] after resolution: queue the pick with its track
/// data. Split off so it can be tested without the network.
async fn enqueue_resolved_autoplay(
    data: &Data,
    guild_id: GuildId,
    call: &Arc<Mutex<Call>>,
    source: SongbirdInput,
    metadata_vec: Vec<NewAuxMetadata>,
) -> Result<TrackHandle, CrackedError> {
    let metadata = metadata_vec.into_iter().next().map(|meta| meta.0);
    // Supplied rather than derived: `enqueue_input` would read it back off the
    // input, which for a lazy source means spawning yt-dlp under the guard.
    // Resolution above already produced the duration.
    let preload = preload_from_metadata(metadata.as_ref());
    // Built into the track, not written in after: see `track_data`. Autoplay
    // has no requester.
    let with_data = track_data(metadata, None);
    let guard = data.lock_queue(guild_id, PlaybackOwner::Free).await?;
    Ok(enqueue_input_back(&guard, call, source, with_data, preload).await)
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

/// Tell the channel autoplay has been switched off, in as few words as that
/// takes. The reason is in the logs.
async fn announce_autoplay_off(channel: GenericChannelId, http: Arc<Http>, has_recommender: bool) {
    send_plain(channel, http, autoplay_off_notice(has_recommender)).await;
}

/// "Autoplay off" -- unless this deployment has no recommender at all, which is
/// the one reason worth naming.
fn autoplay_off_notice(has_recommender: bool) -> &'static str {
    if has_recommender {
        AUTOPLAY_STOPPED
    } else {
        AUTOPLAY_NEEDS_MUSICRECO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wording is the user's, exactly, so it is pinned against literals
    /// rather than the constants -- a test comparing a constant to itself
    /// would pass whatever the constant said.
    #[test]
    fn autoplay_off_says_nothing_more_than_it_needs_to() {
        assert_eq!(autoplay_off_notice(true), "Autoplay off");
        assert_eq!(
            autoplay_off_notice(false),
            "Autoplay needs crack-musicreco!"
        );
    }

    /// Autoplay's pick goes into the queue carrying what it resolved to, so
    /// `/queue`, `/nowplaying` and the next refill can read it.
    #[tokio::test]
    async fn an_autoplay_pick_is_queued_with_the_metadata_it_resolved_to() {
        let data = Data(Arc::new(crate::DataInner::default()));
        let guild_id = GuildId::new(1);
        let call = Arc::new(Mutex::new(Call::standalone(
            guild_id,
            serenity::all::UserId::new(2),
        )));
        let resolved = vec![NewAuxMetadata(songbird::input::AuxMetadata {
            title: Some("Want You Bad".to_owned()),
            ..Default::default()
        })];

        let track = enqueue_resolved_autoplay(
            &data,
            guild_id,
            &call,
            songbird::input::File::new("/nonexistent/pick.opus").into(),
            resolved,
        )
        .await
        .expect("queued");

        let meta = get_track_handle_metadata(&track).await.expect("metadata");
        assert_eq!(meta.title.as_deref(), Some("Want You Bad"));
    }
}
