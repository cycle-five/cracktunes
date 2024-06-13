use crate::{
    commands::{cmd_check_music, sub_help as help},
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    Context, Error,
};

/// Toggle autopause.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    subcommands("help"),
    guild_only,
    check = "cmd_check_music"
)]
pub async fn autopause(ctx: Context<'_>) -> Result<(), Error> {
    autopause_internal(ctx).await
}

/// Toggle autopause internal.
#[cfg(not(tarpaulin_include))]
pub async fn autopause_internal(ctx: Context<'_>) -> Result<(), Error> {
    use crate::messaging::interface::send_message;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

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

    Ok(())
}
