use crate::commands::CrackedError;
use crate::guild::settings::{GuildSettings, DEFAULT_PREFIX};
use crate::Data;
use crate::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};
use serenity::all::{Channel, GuildId};
use serenity::model::id::ChannelId;

/// Set a log channel for a specific guild.
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn log_channel_for_guild(
    ctx: Context<'_>,
    #[description = "GuildId to set logging for"] guild_id: GuildId,
    #[description = "ChannelId to sendlogs"] channel_id: ChannelId,
    #[description = "Type of logs to send"] _log_type: String,
) -> Result<(), Error> {
    // set_all_log_channel_old_data(ctx.serenity_context().data.clone(), guild_id, channel_id).await?;
    set_all_log_channel_data(ctx.data(), guild_id, channel_id).await?;

    send_reply(
        &ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
        true,
    )
    .await?;

    Ok(())
}

/// Set a channel to send all logs.
#[poise::command(
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn all_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send all logs"] channel: Option<Channel>,
    #[description = "ChannelId to send all logs"] channel_id: Option<
        serenity::model::id::ChannelId,
    >,
    #[flag]
    #[description = "Show the help menu for this command"]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    let channel_id = if let Some(channel) = channel {
        channel.id()
    } else if let Some(channel_id) = channel_id {
        channel_id
    } else {
        return Err(Box::new(CrackedError::Other("No channel provided")));
    };
    let guild_id = ctx.guild_id().unwrap();

    set_all_log_channel_data(ctx.data(), guild_id, channel_id).await?;

    send_reply(
        &ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
        true,
    )
    .await?;

    Ok(())
}

#[cfg(not(tarpaulin_include))]
/// Internal function to set a log channel for a specific guild.
pub async fn set_all_log_channel_data(
    data: &Data,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), Error> {
    data.guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.set_all_log_channel(channel_id.into());
        })
        .or_insert({
            GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None)
                .set_all_log_channel(channel_id.into())
                .to_owned()
        });
    Ok(())
}
