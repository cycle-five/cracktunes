use crate::commands::CrackedError;
use crate::db::GuildEntity;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};
use crate::poise_ext::ContextExt;
use crate::guild::operations::GuildSettingsOperations;

/// Internal set premium function without #command macro.
#[cfg(not(tarpaulin_include))]
pub async fn set_premium_internal(ctx: Context<'_>, premium: bool) -> Result<(), CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    ctx.data().set_premium(guild_id, premium).await;
    let pool = ctx.get_db_pool()?;
    GuildEntity::update_premium(&pool, guild_id.get() as i64, premium)
        .await
        .unwrap();
    let _ = send_reply(&ctx, CrackedMessage::Premium(premium), true).await?;
    Ok(())
}

/// Set the premium status of the guild.
#[poise::command(category = "Settings", prefix_command, owners_only)]
#[cfg(not(tarpaulin_include))]
pub async fn premium(
    ctx: Context<'_>,
    #[description = "True or false setting for premium."] premium: bool,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    set_premium_internal(ctx, premium).await.map_err(Into::into)
}
