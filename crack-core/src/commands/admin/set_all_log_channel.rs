use crate::{
    commands::set_all_log_channel_data, messaging::message::CrackedMessage,
    utils::send_response_poise, Context, Error,
};
use serenity::all::Channel;

/// Set the join-leave log channel.
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
