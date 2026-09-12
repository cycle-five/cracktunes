use crate::guild::operations::GuildSettingsOperations;
use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply, Context, Error,
};
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
    #[description = "GenericChannelId of Channel to respond to music commands in."] channel_id: Option<
        serenity::model::id::GenericChannelId,
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

    // 🔑 Before mutating: make sure what is in memory came from Postgres. A
    // guild whose boot load failed holds defaults, and `save()` below is a
    // full-row upsert that would write them over its stored row.
    ctx.data().ensure_settings_loaded(guild_id).await?;

    let data = ctx.data();
    let _ = data.set_music_channel(guild_id, channel_id).await;

    let opt_settings = data.guild_settings_map.read().await.clone();
    let settings = opt_settings.get(&guild_id);

    // FIXME: Do this with the async work queue.
    // 🪤 `unwrap()` here panicked on a tokio worker whenever there was no pool.
    let pg_pool = ctx
        .data()
        .database_pool
        .clone()
        .ok_or(CrackedError::Other("No database pool"))?;
    if let Some(s) = settings {
        s.save(&pg_pool).await?;
    }

    let _ = send_reply(
        &ctx,
        CrackedMessage::Other(format!("Music channel set to {}", channel_id)),
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

    // 🔑 Before mutating: make sure what is in memory came from Postgres. A
    // guild whose boot load failed holds defaults, and `save()` below is a
    // full-row upsert that would write them over its stored row.
    ctx.data().ensure_settings_loaded(guild_id).await?;

    let data = ctx.data();
    let _ = data.add_denied_music_user(guild_id, user).await;

    let opt_settings = data.guild_settings_map.read().await.clone();
    let settings = opt_settings.get(&guild_id);

    // 🪤 `unwrap()` here panicked on a tokio worker whenever there was no pool.
    let pg_pool = ctx
        .data()
        .database_pool
        .clone()
        .ok_or(CrackedError::Other("No database pool"))?;
    if let Some(s) = settings {
        s.save(&pg_pool).await?;
    }

    let _ = send_reply(
        &ctx,
        CrackedMessage::Other(format!("Denied user set to {}", user)),
        true,
    )
    .await?;

    Ok(())
}
