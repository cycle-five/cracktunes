use crate::{
    commands::cmd_check_music,
    messaging::courier,
    music::ops::{self, OpCx},
    CrackedError, {Context, Error},
};

/// Resume the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    resume_internal(ctx).await
}

/// Internal function to resume the current track.
pub async fn resume_internal(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let resumed = ops::resume(&cx)
        .await
        .map_err(CrackedError::from)?
        .settle_now(&cx)
        .await;
    courier::reply(ctx, resumed.message()).await?;
    Ok(())
}
