use crate::guild::operations::GuildSettingsOperations;
use crate::http_utils::SendMessageParams;
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::{Context, Error};

#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    aliases("get_idle_timeout")
)]
pub async fn idle_timeout(
    ctx: Context<'_>,
    #[flag]
    #[description = "Shows the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    idle_timeout_internal(ctx).await
}

/// Get the idle timeout for the bot
pub async fn idle_timeout_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or(crate::commands::CrackedError::NoGuildId)?;

    let idle_timeout = ctx.data().get_timeout(guild_id).await;

    let params = SendMessageParams::new(CrackedMessage::Other(
        format!("Idle timeout: {:?}s", idle_timeout).to_string(),
    ));
    ctx.send_message(params)
        .await
        .map_err(Into::into)
        .map(|_| ())
}
