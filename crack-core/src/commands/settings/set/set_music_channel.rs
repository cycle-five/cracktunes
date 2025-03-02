use crate::guild::operations::GuildSettingsOperations;
use crate::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};
use crack_types::errors::CrackedError;
use serenity::all::Channel;

#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn music_channel(
    ctx: Context<'_>,
    #[description = "Channel to respond to music commands in."] channel: Option<Channel>,
    #[description = "ChannelId of Channel to respond to music commands in."] channel_id: Option<
        serenity::model::id::ChannelId,
    >,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
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
    let () = data.set_music_channel(guild_id, channel_id).await;

    let opt_settings = data.guild_settings_map.read().await.clone();
    let settings = opt_settings.get(&guild_id);

    // FIXME: Do this with the async work queue.
    let pg_pool = ctx.data().database_pool.clone().unwrap();
    settings.map(|s| s.save(&pg_pool)).unwrap().await?;

    let _ = send_reply(
        &ctx,
        CrackedMessage::Other(format!("Music channel set to {channel_id}")),
        true,
    )
    .await?;

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

    let _ = send_reply(
        &ctx,
        CrackedMessage::Other(format!("Denied user set to {user}")),
        true,
    )
    .await?;

    Ok(())
}
