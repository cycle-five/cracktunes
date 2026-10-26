//! Is the bot connected, and which voice channel is a user in.

use crack_common::errors::{CrackedError, Error};
use serenity::all::{ChannelId, Guild, GuildId, UserId};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The guild's Call, but only if it actually carries a voice connection --
/// and, when `expect` is given, only if that connection is to that channel.
///
/// 🪤 `Songbird::get` is not a connection test. songbird registers the Call
/// when a join is *attempted*, before the gateway handshake, so a join that
/// timed out still hands one back. Three call sites used to treat that as
/// success: `do_join` registered handlers and sent a "Summoned" embed for a
/// bot in no voice channel, and `summon_internal` and `get_call_or_join_author`
/// skipped the join -- and therefore the permission gate -- entirely.
///
/// `Songbird::get` is banned in `clippy.toml` (#507). It used to be called
/// from ~17 sites, eight of which spelled their result
/// `CrackedError::NotConnected` while asking a question that cannot establish
/// it. Those now come here. The few that genuinely want "is a Call
/// registered" (debug dumps, `/gp end`'s cleanup) carry an `#[allow]` saying
/// why.
///
/// 🪤 `expect` matters because "connected" is not "connected to the channel
/// you asked for". A concurrent `/gp` resume can land a join on another
/// channel between a failed join and this check, and without the
/// comparison `do_join` would announce "Summoned <#B>" for a bot in A.
pub async fn connected_call(
    manager: &songbird::Songbird,
    guild_id: GuildId,
    expect: Option<ChannelId>,
) -> Option<Arc<Mutex<Call>>> {
    // The one place the raw question is asked: this function exists to turn
    // it into the right one.
    #[allow(clippy::disallowed_methods)]
    let call = manager.get(guild_id)?;
    let (connected, here) = {
        let handler = call.lock().await;
        (
            handler.current_connection().is_some(),
            handler.current_channel(),
        )
    };
    match (connected, here, expect) {
        (false, _, _) => None,
        (true, _, None) => Some(call),
        (true, Some(here), Some(want)) => (here.get() == want.get()).then_some(call),
        (true, None, Some(_)) => None,
    }
}

/// Get the voice channel a user is in within a guild.
/// # Errors
/// * If the user is not in a voice channel in the guild.
pub fn get_voice_channel_for_user(guild: &Guild, user_id: &UserId) -> Result<ChannelId, Error> {
    guild
        .voice_states
        .get(user_id)
        .and_then(|voice_state| voice_state.channel_id)
        .ok_or(CrackedError::NotConnected.into())
}

#[cfg(test)]
mod test {
    use serenity::all::{Guild, UserId};

    #[test]
    fn test_get_voice_channel_for_user() {
        let guild = Guild::default();
        let user_id = UserId::new(1);

        assert!(super::get_voice_channel_for_user(&guild, &user_id).is_err());
    }
}
