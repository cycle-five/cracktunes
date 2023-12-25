use crate::{
    commands::set_all_log_channel_data, messaging::message::CrackedMessage,
    utils::send_response_poise, Context, Error,
};
use serenity::all::{Channel, GuildId};
use serenity::model::id::ChannelId;

/// Set a log channel for a specific guild.
#[poise::command(prefix_command, owners_only)]
pub async fn set_log_channel_for_guild(
    ctx: Context<'_>,
    #[description = "GuildId to set logging for"] guild_id: GuildId,
    #[description = "ChannelId to sendlogs"] channel_id: ChannelId,
    #[description = "Type of logs to send"] _log_type: String,
) -> Result<(), Error> {
    // set_all_log_channel_old_data(ctx.serenity_context().data.clone(), guild_id, channel_id).await?;
    set_all_log_channel_data(ctx.data(), guild_id, channel_id).await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}

/// Set a channel to send all logs.
#[poise::command(prefix_command, owners_only)]
pub async fn set_all_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send all logs"] channel: Option<Channel>,
    #[description = "ChannelId to send all logs"] channel_id: Option<
        serenity::model::id::ChannelId,
    >,
) -> Result<(), Error> {
    let channel_id = if let Some(channel) = channel {
        channel.id()
    } else {
        channel_id.unwrap()
    };
    let guild_id = ctx.guild_id().unwrap();

    // set_all_log_channel_old_data(ctx.serenity_context().data.clone(), guild_id, channel_id).await?;
    set_all_log_channel_data(ctx.data(), guild_id, channel_id).await?;

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}
