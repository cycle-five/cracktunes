use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::{Context, ContextExt, Error};

/// Get recently played tracks form the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn playlog(ctx: Context<'_>) -> Result<(), Error> {
    playlog_(ctx).await
}

/// Get recently played tracks for the guild.
#[cfg(not(tarpaulin_include))]
pub async fn playlog_(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let last_played = ctx.get_last_played().await?;

    let msg = send_response_poise(ctx, CrackedMessage::PlayLog(last_played), true).await?;
    ctx.data().add_msg_to_cache(guild_id, msg).await;

    Ok(())
}

/// Get recently played tracks for the calling user in the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn myplaylog(ctx: Context<'_>) -> Result<(), Error> {
    myplaylog_(ctx).await
}

#[cfg(not(tarpaulin_include))]
pub async fn myplaylog_(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;

    let last_played = ctx.get_last_played_by_user(user_id).await?;

    let msg = send_response_poise(ctx, CrackedMessage::PlayLog(last_played), true).await?;
    ctx.data().add_msg_to_cache(guild_id, msg).await;

    Ok(())
}
