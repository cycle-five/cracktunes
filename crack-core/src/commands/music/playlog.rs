use crate::db::PlayLog;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::{Context, Error};

/// Get recently played tracks.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn playlog(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let last_played = PlayLog::get_last_played(
        ctx.data().database_pool.as_ref().unwrap(),
        None,
        Some(guild_id.get() as i64),
    )
    .await?;

    let msg = send_response_poise(ctx, CrackedMessage::PlayLog(last_played), true).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}

/// Get recently played tracks.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn myplaylog(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let user_id = ctx.author().id;

    let last_played = PlayLog::get_last_played(
        ctx.data().database_pool.as_ref().unwrap(),
        Some(user_id.get() as i64),
        Some(guild_id.get() as i64),
    )
    .await?;

    let msg = send_response_poise(ctx, CrackedMessage::PlayLog(last_played), true).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
