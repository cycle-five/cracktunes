use self::serenity::async_trait;
use poise::serenity_prelude as serenity;
use songbird::{tracks::PlayMode, Event, EventContext, EventHandler};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use crack_types::messaging::messages::IDLE_ALERT;

/// Handler for the idle event.
pub struct IdleHandler {
    pub serenity_ctx: Arc<serenity::Context>,
    pub guild_id: serenity::GuildId,
    pub channel_id: serenity::ChannelId,
    pub limit: usize,
    pub count: Arc<AtomicUsize>,
    pub no_timeout: Arc<AtomicBool>,
}
use songbird::error::JoinError;

/// TODO: Add metrics
/// Implement handler for the idle event.
#[cfg(not(tarpaulin_include))]
#[async_trait]
impl EventHandler for IdleHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let manager = &self.serenity_ctx.data::<crate::Data>().songbird;
        let EventContext::Track(track_list) = ctx else {
            return None;
        };

        // tracing::warn!("IdleHandler: {:?}", len(track_list));
        // tracing::warn!("Guild ID: {:?}", self.guild_id);

        // let handler = match manager.get(self.guild_id) {
        //     Some(call) => call,
        //     None => return Some(Event::Cancel),
        // };

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
        // tracing::warn!(
        //     "is_playing: {:?}, time_not_playing: {:?}",
        //     bot_is_playing,
        //     self.count.load(Ordering::Relaxed)
        // );

        if !self.no_timeout.load(Ordering::Relaxed)
            && self.limit > 0
            && self.count.fetch_add(60, Ordering::Relaxed) >= self.limit
        {
            match manager.remove(self.guild_id).await {
                Ok(()) => {
                    match self
                        .channel_id
                        .say(&self.serenity_ctx.http, IDLE_ALERT)
                        .await
                    {
                        Ok(_) => {},
                        Err(e) => {
                            tracing::error!("Error sending idle alert: {:?}", e);
                            return Some(Event::Cancel);
                        },
                    };
                },
                Err(JoinError::NoCall) => {
                    tracing::warn!("No call found for guild: {:?}", self.guild_id);
                    return Some(Event::Cancel);
                },
                Err(e) => {
                    tracing::error!("Error removing bot from voice channel: {:?}", e);
                    return Some(Event::Cancel);
                },
            };
        }
        None
    }
}
