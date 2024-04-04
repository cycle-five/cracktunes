use ::serenity::{
    all::{ChannelId, UserId},
    async_trait,
    builder::EditMessage,
    http::Http,
    model::id::GuildId,
};
use songbird::{input::AuxMetadata, tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

use crate::{
    commands::{
        doplay_utils::enqueue_track_pgwrite_asdf, forget_skip_votes, send_now_playing,
        MyAuxMetadata,
    },
    db::PlayLog,
    errors::{verify, CrackedError},
    interface::{build_nav_btns, create_queue_embed},
    messaging::messages::SPOTIFY_AUTH_FAILED,
    sources::spotify::{Spotify, SPOTIFY},
    utils::{calculate_num_pages, forget_queue_message},
    Data, Error,
};

pub struct TrackEndHandler {
    pub guild_id: GuildId,
    pub data: Data,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
}

pub struct ModifyQueueHandler {
    pub guild_id: GuildId,
    pub data: Data,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
}

/// Event handler to handle the end of a track.
#[async_trait]
impl EventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        tracing::error!("TrackEndHandler");
        let autoplay = {
            self.data
                .guild_cache_map
                .lock()
                .unwrap()
                .entry(self.guild_id)
                .or_default()
                .autoplay
        };
        tracing::error!("Autoplay: {}", autoplay);

        let (autopause, volume) = {
            let settings = self.data.guild_settings_map.read().unwrap().clone();
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

        self.call.lock().await.queue().modify_queue(|v| {
            if let Some(track) = v.front_mut() {
                let _ = track.set_volume(volume);
            };
        });
        tracing::error!("Set volume");

        if autopause {
            tracing::error!("Pausing");
            self.call.lock().await.queue().pause().ok();
        } else {
            tracing::error!("Not pausing");
        }

        tracing::error!("Forgetting skip votes");
        // FIXME
        match forget_skip_votes(&self.data, self.guild_id).await {
            Ok(_) => tracing::warn!("Forgot skip votes"),
            Err(e) => tracing::warn!("Error forgetting skip votes: {}", e),
        };

        let (chan_id, _chan_name, MyAuxMetadata::Data(metadata), cur_position) = {
            let (sb_chan_id, my_metadata, cur_pos) = {
                let (channel, track) = {
                    let handler = self.call.lock().await;
                    let channel = handler.current_channel();
                    let track = handler.queue().current().clone();
                    (channel, track)
                };
                let chan_id = channel.map(|c| ChannelId::new(c.0.get())).unwrap();
                match (track, autoplay) {
                    (None, false) => (
                        channel,
                        MyAuxMetadata::Data(AuxMetadata::default()),
                        Duration::from_secs(0),
                    ),
                    (None, true) => {
                        let spotify = SPOTIFY.lock().await;
                        let spotify =
                            verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))
                                .unwrap();
                        // Get last played tracks from the db
                        let last_played = PlayLog::get_last_played(
                            self.data.database_pool.as_ref().unwrap(),
                            None,
                            Some(self.guild_id.get() as i64),
                        )
                        .await
                        .unwrap_or_default();
                        let res_rec =
                            Spotify::get_recommendations(spotify, last_played.clone()).await;
                        let (rec, msg) = match res_rec {
                            Ok(rec) => {
                                // let msg0 = format!(
                                //     "Previously played: \n{}",
                                //     last_played.clone().join("\n")
                                // );
                                let msg1 =
                                    format!("Autoplaying (/autoplay or /stop to stop): {}", rec[0]);
                                (rec, msg1)
                            }
                            Err(e) => {
                                let msg = format!("Error: {}", e);
                                let rec = vec![];
                                (rec, msg)
                            }
                        };
                        // let msg = format!("Rec: {:?}", rec);
                        tracing::warn!("{}", msg);
                        let msg = chan_id.say(&self.http, msg).await.unwrap();
                        self.data.add_msg_to_cache(self.guild_id, msg);
                        let query = match Spotify::search(spotify, &rec[0]).await {
                            Ok(query) => query,
                            Err(e) => {
                                let msg = format!("Error: {}", e);
                                tracing::warn!("{}", msg);
                                // chan_id.say(&self.http, msg).await.unwrap();
                                return None;
                            }
                        };
                        let q = enqueue_track_pgwrite_asdf(
                            self.data.database_pool.as_ref().unwrap(),
                            self.guild_id,
                            chan_id,
                            UserId::new(1),
                            &self.http,
                            &self.call,
                            &query,
                        )
                        .await;
                        let (my_metadata, pos) = match q {
                            Ok(tracks) => {
                                tracks.first().map(|t| {
                                    let _ = t.set_volume(volume);
                                });
                                let (my_metadata, pos) =
                                    extract_track_metadata(&tracks[0]).await.unwrap_or_default();
                                // match extract_track_metadata(&tracks[0]).await {
                                //     Ok((my_metadata, pos)) => (my_metadata, pos),
                                //     Err(e) => {
                                //         let msg = format!("Error: {}", e);
                                //         tracing::warn!("{}", msg);
                                //         (
                                //             MyAuxMetadata::Data(AuxMetadata::default()),
                                //             Duration::from_secs(0),
                                //         )
                                //     }
                                // };
                                (my_metadata, pos)
                            }
                            Err(e) => {
                                let msg = format!("Error: {}", e);
                                tracing::warn!("{}", msg);

                                (
                                    MyAuxMetadata::Data(AuxMetadata::default()),
                                    Duration::from_secs(0),
                                )
                            }
                        };
                        (channel, my_metadata, pos)
                    }
                    (Some(track), _) => {
                        let _ = track.set_volume(volume);
                        let (my_metadata, pos) =
                            extract_track_metadata(&track).await.unwrap_or_default();

                        (channel, my_metadata, pos)
                    }
                }
            };
            let chan_id = sb_chan_id.map(|id| ChannelId::new(id.0.get())).unwrap();
            let chan_name = chan_id.name(&self.http).await.unwrap();
            (chan_id, chan_name, my_metadata, cur_pos)
        };

        tracing::warn!("Sending now playing message");

        match send_now_playing(
            chan_id,
            self.http.clone(),
            self.call.clone(),
            Some(cur_position),
            Some(metadata),
        )
        .await
        {
            Ok(message) => {
                self.data.add_msg_to_cache(self.guild_id, message);
                tracing::warn!("Sent now playing message");
            }
            Err(e) => tracing::warn!("Error sending now playing message: {}", e),
        }

        None
    }
}

async fn extract_track_metadata(track: &TrackHandle) -> Result<(MyAuxMetadata, Duration), Error> {
    let pos = track.get_info().await.unwrap().position;
    let track_clone = track.clone();
    let mutex_guard = track_clone.typemap().read().await;
    let my_metadata = mutex_guard
        .get::<crate::commands::MyAuxMetadata>()
        .unwrap()
        .clone();
    Ok((my_metadata, pos))
}

/// Event handler to set the volume of the playing track to the volume
/// set in the guild settings after a queue modification.
#[async_trait]
impl EventHandler for ModifyQueueHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let (queue, vol) = {
            let handler = self.call.lock().await;
            let queue = handler.queue().current_queue().clone();
            let settings = self.data.guild_settings_map.read().unwrap().clone();
            let vol = settings
                .get(&self.guild_id)
                .map(|guild_settings| guild_settings.volume);
            (queue, vol)
        };

        vol.map(|vol| queue.first().map(|track| track.set_volume(vol).unwrap()));
        update_queue_messages(&self.http, &self.data, &queue, self.guild_id).await;

        None
    }
}

/// This function goes through all the active "queue" messages that are still
/// being updated and updates them with the current.
pub async fn update_queue_messages(
    http: &Http,
    data: &Data,
    tracks: &[TrackHandle],
    guild_id: GuildId,
) {
    let cache_map = data.guild_cache_map.lock().unwrap().clone();

    let mut messages = match cache_map.get(&guild_id) {
        Some(cache) => cache.queue_messages.clone(),
        None => return,
    };

    for (message, page_lock) in messages.iter_mut() {
        // has the page size shrunk?
        let num_pages = calculate_num_pages(tracks);
        let page = *page_lock.read().unwrap();
        let page_val = usize::min(page, num_pages - 1);
        *page_lock.write().unwrap() = page_val;

        let embed = create_queue_embed(tracks, page_val).await;

        let edit_message = message
            .edit(
                &http,
                EditMessage::new()
                    .embed(embed)
                    .components(build_nav_btns(page_val, num_pages)),
            )
            .await;

        if edit_message.is_err() {
            forget_queue_message(data, message, guild_id).await.ok();
        };
    }
}
