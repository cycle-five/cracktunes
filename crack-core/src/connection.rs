use self::serenity::model::{
    guild::Guild,
    id::{ChannelId, UserId},
};
use crate::{errors::CrackedError, Error};
use poise::serenity_prelude as serenity;

pub enum Connection {
    User(ChannelId),
    Bot(ChannelId),
    Mutual(ChannelId, ChannelId),
    Separate(ChannelId, ChannelId),
    Neither,
}

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

pub fn get_voice_channel_for_user(guild: &Guild, user_id: &UserId) -> Result<ChannelId, Error> {
    guild
        .voice_states
        .get(user_id)
        .and_then(|voice_state| voice_state.channel_id)
        .ok_or(CrackedError::NotConnected.into())
}
