use crate::{
    commands::cmd_check_music,
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
    let done = ops::pause(&cx).await.map_err(CrackedError::from)?;
    let msg = done.outcome().message();
    done.reply_then_settle(ctx, &cx, msg).await?;
    Ok(())
}
