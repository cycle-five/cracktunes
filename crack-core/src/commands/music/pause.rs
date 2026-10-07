use crate::{
    commands::cmd_check_music,
    messaging::courier,
    music::ops::{self, OpCx},
    CrackedError, {Context, Error},
};

/// Pause the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let paused = ops::pause(&cx)
        .await
        .map_err(CrackedError::from)?
        .settle_now(&cx)
        .await;
    courier::reply(ctx, paused.message()).await?;
    Ok(())
}
