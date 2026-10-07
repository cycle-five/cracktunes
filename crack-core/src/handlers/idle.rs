use self::serenity::async_trait;
use poise::serenity_prelude as serenity;
use songbird::{tracks::PlayMode, Event, EventContext, EventHandler};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use crate::messaging::courier::{self, Destination};
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::IDLE_ALERT;
use crate::messaging::render::RenderCx;

/// Handler for the idle event.
pub struct IdleHandler {
    pub serenity_ctx: Arc<serenity::Context>,
    pub guild_id: serenity::GuildId,
    pub channel_id: serenity::GenericChannelId,
    pub limit: usize,
    pub count: Arc<AtomicUsize>,
    pub no_timeout: Arc<AtomicBool>,
}
use songbird::error::JoinError;

/// Whether an idle bot leaves now. `count` is the idle seconds counted before
/// this tick. A premium server (`no_timeout`) never times out, so it never sees
/// `IDLE_ALERT`; a `limit` of 0 means the timeout is off.
#[must_use]
pub fn times_out(no_timeout: bool, limit: usize, count: usize) -> bool {
    !no_timeout && limit > 0 && count >= limit
}

/// TODO: Add metrics
/// Implement handler for the idle event.
#[cfg(not(tarpaulin_include))]
#[async_trait]
impl EventHandler for IdleHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let data = self.serenity_ctx.data::<crate::Data>();
        let manager = &data.songbird;
        let EventContext::Track(track_list) = ctx else {
            return None;
        };

        // A `/gp` game is silent on purpose between songs: a submission window can
        // run for ten minutes with nothing playing, which otherwise reads as an
        // idle bot and disconnects mid-round, taking the game with it.
        if data.gp_is_active(self.guild_id) {
            self.count.store(0, Ordering::Relaxed);
            return None;
        }

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

        let no_timeout = self.no_timeout.load(Ordering::Relaxed);
        // Count only while a timeout can happen, as before: premium and a zero
        // limit leave the counter alone.
        let count = if no_timeout || self.limit == 0 {
            0
        } else {
            self.count.fetch_add(60, Ordering::Relaxed)
        };
        if times_out(no_timeout, self.limit, count) {
            match crate::music::disconnect::disconnect(
                &data,
                manager,
                self.guild_id,
                crate::music::audit::Actor::bot(crate::music::audit::BotReason::IdleTimeout),
            )
            .await
            {
                Ok(_) => {
                    crate::messaging::status::show_finished(
                        &data,
                        self.serenity_ctx.http.clone(),
                        self.serenity_ctx.cache.clone(),
                        self.guild_id,
                    )
                    .await;
                    let transport = crate::messaging::status::DiscordTransport {
                        http: self.serenity_ctx.http.clone(),
                        cache: self.serenity_ctx.cache.clone(),
                    };
                    let sent = courier::post(
                        &data,
                        &transport,
                        Destination::Channel(self.channel_id),
                        &CrackedMessage::Other(IDLE_ALERT.to_owned()),
                        &RenderCx::now(),
                    )
                    .await;
                    if sent.is_none() {
                        // `post` has logged why.
                        return Some(Event::Cancel);
                    }
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

#[cfg(test)]
mod test {
    use super::times_out;

    #[test]
    fn premium_never_times_out() {
        assert!(!times_out(true, 600, 0));
        assert!(!times_out(true, 600, 600));
        assert!(!times_out(true, 600, usize::MAX));
    }

    #[test]
    fn free_times_out_once_idle_reaches_the_limit() {
        assert!(!times_out(false, 600, 540));
        assert!(times_out(false, 600, 600));
        assert!(times_out(false, 600, 660));
    }

    #[test]
    fn a_zero_limit_never_times_out() {
        assert!(!times_out(false, 0, 10_000));
    }
}
