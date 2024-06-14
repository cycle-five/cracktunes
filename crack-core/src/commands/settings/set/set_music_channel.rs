use crate::guild::operations::GuildSettingsOperations;
use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply, Context,
    Error,
};
use serenity::all::Channel;

#[poise::command(prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn music_channel(
    ctx: Context<'_>,
    #[description = "Channel to respond to music commands in."] channel: Option<Channel>,
    #[description = "ChannelId of Channel to respond to music commands in."] channel_id: Option<
        serenity::model::id::ChannelId,
    >,
) -> Result<(), Error> {
    if channel.is_none() && channel_id.is_none() {
        return Err(CrackedError::Other("Must provide either a channel or a channel id").into());
    }
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let channel_id = if let Some(channel) = channel {
        channel.id()
    } else {
        channel_id.unwrap()
    };

    let data = ctx.data();
    let _ = data.set_music_channel(guild_id, channel_id).await;

    let opt_settings = data.guild_settings_map.read().await.clone();
    let settings = opt_settings.get(&guild_id);

    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    let msg = send_reply(
        ctx,
        CrackedMessage::Other(format!("Music channel set to {}", channel_id)),
        true,
    )
    .await?;
    data.add_msg_to_cache(guild_id, msg).await;

    Ok(())
}

use poise::serenity_prelude as serenity;

#[poise::command(prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn music_denied_user(
    ctx: Context<'_>,
    #[description = "User to deny music commands to."] user: serenity::UserId,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let data = ctx.data();
    let _ = data.add_denied_music_user(guild_id, user).await;

    let opt_settings = data.guild_settings_map.read().await.clone();
    let settings = opt_settings.get(&guild_id);

    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    let msg = send_reply(
        ctx,
        CrackedMessage::Other(format!("Denied user set to {}", user)),
        true,
    )
    .await?;
    data.add_msg_to_cache(guild_id, msg).await;

    Ok(())
}
