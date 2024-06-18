use crate::{poise_ext::ContextExt, Context, Error};

/// Invite link for the bot
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, prefix_command)]
pub async fn invite(ctx: Context<'_>) -> Result<(), Error> {
    invite_internal(ctx).await
}

/// Testable internal function for invite.
pub async fn invite_internal(ctx: Context<'_>) -> Result<(), Error> {
    let _ = ctx.send_invite_link().await?;
    Ok(())
}
