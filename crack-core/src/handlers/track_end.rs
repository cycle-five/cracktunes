use self::serenity::{async_trait, http::Http, model::id::GuildId, Mutex};
use poise::serenity_prelude::{self as serenity, ChannelId};
use songbird::{tracks::TrackHandle, Call, Event, EventContext, EventHandler};
use std::sync::Arc;

use crate::{
    commands::music::queue::{
        build_nav_btns, calculate_num_pages, create_queue_embed, forget_queue_message,
    },
    commands::music::{send_now_playing, voteskip::forget_skip_votes},
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
        let settings = self.data.guild_settings_map.lock().unwrap().clone();

        let autopause = settings
            .get(&self.guild_id)
            .map(|guild_settings| guild_settings.autopause)
            .unwrap_or_default();
        let volume = settings
            .get(&self.guild_id)
            .map(|guild_settings| guild_settings.volume)
            .unwrap_or(crate::guild::settings::DEFAULT_VOLUME_LEVEL);

        let handler = self.call.lock().await;
        let queue = handler.queue();
        queue.modify_queue(|v| {
            if let Some(track) = v.front_mut() {
                let _ = track.set_volume(volume);
            };
        });
        if autopause {
            queue.pause().ok();
        }

        forget_skip_votes(&self.data, self.guild_id).await.ok();

        if let Some(channel) = handler.current_channel() {
            tracing::warn!("Sending now playing message");
            let chan_id = ChannelId(channel.0);

            send_now_playing(chan_id, self.http.clone(), self.call.clone())
                .await
                .ok();
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
        let handler = self.call.lock().await;
        let queue = handler.queue().current_queue();
        let settings = self.data.guild_settings_map.lock().unwrap().clone();
        let vol = settings
            .get(&self.guild_id)
            .map(|guild_settings| guild_settings.volume);
        drop(handler);

        vol.map(|vol| queue.first().map(|track| track.set_volume(vol)));
        update_queue_messages(&self.http, &self.data, &queue, self.guild_id).await;

        None
    }
}

pub async fn update_queue_messages(
    http: &Arc<Http>,
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
        let mut page = page_lock.write().await;
        *page = usize::min(*page, num_pages - 1);

        let embed = create_queue_embed(tracks, *page);

        let edit_message = message
            .edit(&http, |edit| {
                edit.set_embed(embed);
                edit.components(|components| build_nav_btns(components, *page, num_pages))
            })
            .await;

        if edit_message.is_err() {
            forget_queue_message(data, message, guild_id).await.ok();
        };
    }
}
