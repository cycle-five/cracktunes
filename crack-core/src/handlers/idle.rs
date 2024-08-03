use self::serenity::{async_trait, http::Http};
use poise::serenity_prelude as serenity;
use songbird::{tracks::PlayMode, Event, EventContext, EventHandler};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use crate::messaging::messages::IDLE_ALERT;

/// Handler for the idle event.
pub struct IdleHandler {
    pub http: Arc<Http>,
    //pub manager: Arc<Songbird>,
    pub serenity_ctx: Arc<serenity::Context>,
    pub channel_id: serenity::ChannelId,
    pub guild_id: Option<serenity::GuildId>,
    pub limit: usize,
    pub count: Arc<AtomicUsize>,
    pub no_timeout: Arc<AtomicBool>,
}

/// Implement handler for the idle event.
#[cfg(not(tarpaulin_include))]
#[async_trait]
impl EventHandler for IdleHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let manager = songbird::get(&self.serenity_ctx).await?;
        let EventContext::Track(track_list) = ctx else {
            return None;
        };

        tracing::warn!("IdleHandler: {:?}", track_list);
        tracing::warn!("Guild ID: {:?}", self.guild_id);

        // looks like the track list isn't ordered here, so the first track in the list isn't
        // guaranteed to be the first track in the actual queue, so search the entire list
        let bot_is_playing = track_list
            .iter()
            .any(|&(track_state, _track_handle)| matches!(track_state.playing, PlayMode::Play));

        // if there's a track playing, then reset the counter
        if bot_is_playing {
            self.count.store(0, Ordering::Relaxed);
            return None;
        }
        tracing::warn!(
            "is_playing: {:?}, time_not_playing: {:?}",
            self.count,
            self.count.load(Ordering::Relaxed)
        );

        if !self.no_timeout.load(Ordering::Relaxed)
            && self.limit > 0
            && self.count.fetch_add(5, Ordering::Relaxed) >= self.limit
        {
            let guild_id = self.guild_id?;

            if manager.remove(guild_id).await.is_ok() {
                self.channel_id.say(&self.http, IDLE_ALERT).await.unwrap();
                return Some(Event::Cancel);
            }
        }
        None
    }
}
