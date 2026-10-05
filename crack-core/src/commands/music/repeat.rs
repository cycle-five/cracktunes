use crate::{
    commands::cmd_check_music,
    music::ops::{self, OpCx},
    utils::send_reply,
    Context, CrackedError, Error,
};

/// Toggle looping of the current track.
#[cfg(not(tarpaulin_include))]
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
#[cfg(not(tarpaulin_include))]
pub async fn repeat_internal(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let repeat = ops::repeat(&cx, None)
        .await
        .map_err(CrackedError::from)?
        .settle_now(&cx)
        .await;
    send_reply(&ctx, repeat.message(), true).await?;
    Ok(())
}
