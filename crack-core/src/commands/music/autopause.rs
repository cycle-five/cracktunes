use crate::{
    errors::CrackedError, guild::operations::GuildSettingsOperations,
    messaging::message::CrackedMessage, Context, Error,
};

/// Toggle autopause.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    autopause_internal(ctx).await
}

use crate::http_utils::SendMessageParams;

/// Toggle autopause internal.
#[cfg(not(tarpaulin_include))]
pub async fn autopause_internal(ctx: Context<'_>) -> Result<(), Error> {
    use crate::messaging::interface::send_message;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    //let prefix = ctx.data().bot_settings.get_prefix();

    let autopause = ctx.data().toggle_autopause(guild_id).await;
    let params = SendMessageParams {
        msg: if autopause {
            CrackedMessage::AutopauseOn
        } else {
            CrackedMessage::AutopauseOff
        },
        ..Default::default()
    };
    send_message(ctx, params).await?;
    // let msg = if autopause {
    //     send_response_poise(ctx, CrackedMessage::AutopauseOn, true)
    // } else {
    //     send_response_poise(ctx, CrackedMessage::AutopauseOff, true)
    // }
    // .await?;
    // ctx.data().add_msg_to_cache(guild_id, msg).await;
    Ok(())
}
