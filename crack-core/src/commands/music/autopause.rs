use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    guild::operations::GuildSettingsOperations,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    poise_ext::PoiseContextExt,
    Context, Error,
};

/// Toggle whether the bot pauses after each song in the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn autopause(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return help::wrapper(ctx).await;
    }
    autopause_internal(ctx).await
}

/// Toggle autopause internal.
#[cfg(not(tarpaulin_include))]
pub async fn autopause_internal(ctx: Context<'_>) -> Result<(), Error> {
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
    ctx.send_message(params).await?;

    Ok(())
}
