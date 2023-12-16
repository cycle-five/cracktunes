use self::serenity::{async_trait, http::Http, model::id::GuildId};
use ::serenity::{all::ChannelId, builder::EditMessage};
use poise::serenity_prelude::{self as serenity};
use songbird::{tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    commands::{forget_skip_votes, send_now_playing},
    utils::{build_nav_btns, calculate_num_pages, create_queue_embed, forget_queue_message},
    Data,
};

pub struct TrackEndHandler {
    pub guild_id: GuildId,
    pub http: Arc<Http>,
    pub call: Arc<Mutex<Call>>,
    pub data: Data,
}

pub struct ModifyQueueHandler {
    pub http: Arc<Http>,
    pub data: Data,
    pub call: Arc<Mutex<Call>>,
    pub guild_id: GuildId,
}

#[async_trait]
impl EventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        tracing::error!("TrackEndHandler");
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

        self.call.lock().await.queue().modify_queue(|v| {
            if let Some(track) = v.front_mut() {
                let _ = track.set_volume(volume);
            };
        });
        tracing::error!("Set volume");
        //let handler = self.call.lock().await;
        //let queue = handler.queue();
        // queue.modify_queue(|v| {
        //     if let Some(track) = v.front_mut() {
        //         let _ = track.set_volume(volume);
        //     };
        // });
        if autopause {
            tracing::error!("Pausing");
            self.call.lock().await.queue().pause().ok();
        } else {
            tracing::error!("Not pausing");
        }

        tracing::error!("Forgetting queue");
        // FIXME
        match forget_skip_votes(&self.data, self.guild_id).await {
            Ok(_) => tracing::warn!("Forgot skip votes"),
            Err(e) => tracing::warn!("Error forgetting skip votes: {}", e),
        };

        if let Some(channel) = self.call.lock().await.current_channel() {
            tracing::warn!("Sending now playing message");
            let chan_id = ChannelId::new(channel.0.get());

            tracing::warn!("Channel id: {}", chan_id.name(&self.http).await.unwrap());

            match send_now_playing(chan_id, self.http.clone(), self.call.clone()).await {
                Ok(_) => tracing::warn!("Sent now playing message"),
                Err(e) => tracing::warn!("Error sending now playing message: {}", e),
            }
        } else {
            tracing::warn!("No channel to send now playing message");
        }
        // drop(handler);
        None
    }
}

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

        vol.map(|vol| queue.first().map(|track| track.set_volume(vol)));
        update_queue_messages(&self.http, &self.data, &queue, self.guild_id).await;

        None
    }
}

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
    // drop(data);

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
