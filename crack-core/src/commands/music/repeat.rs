use crate::music::from_ctx::{OpCxFromCtx, ReplyThenSettle};
use crate::{
    commands::cmd_check_music,
    music::ops::{self, OpCx},
    Context, CrackedError, Error,
};

/// Toggle looping of the current track.
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn repeat(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    repeat_internal(ctx).await
}

/// Internal repeat function.
pub async fn repeat_internal(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let done = ops::repeat(&cx, None).await.map_err(CrackedError::from)?;
    let msg = done.outcome().message();
    done.reply_then_settle(ctx, &cx, msg).await?;
    Ok(())
}
