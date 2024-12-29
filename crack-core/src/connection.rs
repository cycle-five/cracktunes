use self::serenity::model::{
    guild::Guild,
    id::{ChannelId, UserId},
};
use poise::serenity_prelude as serenity;
use crack_types::CrackedError;
use crate::Error;

#[derive(Debug, PartialEq)]
/// Enum for types of voice connection relationships.
pub enum Connection {
    User(ChannelId),
    Bot(ChannelId),
    Mutual(ChannelId, ChannelId),
    Separate(ChannelId, ChannelId),
    Neither,
}

/// Check the voice connection relationship to anopther user_id (bot).
pub fn check_voice_connections(guild: &Guild, user_id: &UserId, bot_id: &UserId) -> Connection {
    let user_channel = get_voice_channel_for_user(guild, user_id).ok();
    let bot_channel = get_voice_channel_for_user(guild, bot_id).ok();

    if let (Some(bot_id), Some(user_id)) = (bot_channel, user_channel) {
        if bot_id == user_id {
            Connection::Mutual(bot_id, user_id)
        } else {
            Connection::Separate(bot_id, user_id)
        }
    } else if let (Some(bot_id), None) = (bot_channel, user_channel) {
        Connection::Bot(bot_id)
    } else if let (None, Some(user_id)) = (bot_channel, user_channel) {
        Connection::User(user_id)
    } else {
        Connection::Neither
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

/// Get the voice channel a user is in within a guild, return a different error than normal
/// for the summoning case.
pub fn get_voice_channel_for_user_summon(
    guild: &Guild,
    user_id: &UserId,
) -> Result<ChannelId, Error> {
    match get_voice_channel_for_user(guild, user_id) {
        Ok(channel_id) => Ok(channel_id),
        Err(_) => {
            tracing::warn!(
                "User {} is not in a voice channel in guild {}",
                user_id,
                guild.id
            );
            Err(CrackedError::WrongVoiceChannel.into())
        },
    }
}

#[cfg(test)]
mod test {
    use super::{check_voice_connections, Connection};
    use poise::serenity_prelude as serenity;
    use serenity::Guild;
    use serenity::UserId;

    #[test]
    fn test_check_voice_connections() {
        let guild = Guild::default();
        let user_id = UserId::new(1);
        let bot_id = UserId::new(2);

        assert_eq!(
            check_voice_connections(&guild, &user_id, &bot_id),
            Connection::Neither
        );
    }

    #[test]
    fn test_get_voice_channel_for_user() {
        let guild = Guild::default();
        let user_id = UserId::new(1);

        assert!(super::get_voice_channel_for_user(&guild, &user_id).is_err());
    }

    #[test]
    fn test_get_voice_channel_for_user_summon() {
        let guild = Guild::default();
        let user_id = UserId::new(1);

        assert!(super::get_voice_channel_for_user_summon(&guild, &user_id).is_err());
    }
}
