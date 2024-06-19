use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{poise_ext::ContextExt, Context, Error};

/// Get recently played tracks form the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn playlog(ctx: Context<'_>) -> Result<(), Error> {
    playlog_internal(ctx).await
}

/// Get recently played tracks for the guild.
#[cfg(not(tarpaulin_include))]
pub async fn playlog_internal(ctx: Context<'_>) -> Result<(), Error> {
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    use crate::utils::create_paged_embed;

    let last_played = ctx.get_last_played().await?;
    let last_played_str = last_played.join("\n");

    let embed = create_paged_embed(
        ctx,
        ctx.author().name.clone(),
        "Playlog".to_string(),
        last_played_str,
        756,
    )
    .await;
    let _ = send_reply(&ctx, CrackedMessage::PlayLog(last_played), true).await?;

    Ok(())
}

/// Get recently played tracks for the calling user in the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn myplaylog(ctx: Context<'_>) -> Result<(), Error> {
    myplaylog_(ctx).await
}

// use crate::commands::CrackedError;
#[cfg(not(tarpaulin_include))]
pub async fn myplaylog_(ctx: Context<'_>) -> Result<(), Error> {
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let user_id = ctx.author().id;

    let last_played = ctx.get_last_played_by_user(user_id).await?;

    let _ = send_reply(&ctx, CrackedMessage::PlayLog(last_played), true).await?;

    Ok(())
}
