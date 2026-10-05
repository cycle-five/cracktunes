use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    music::ops::{self, OpCx},
    utils::send_reply,
    Context, Error,
};

/// Clear the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn clear(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    clear_internal(ctx).await
}

/// Clear the queue, internal.
pub async fn clear_internal(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let done = ops::clear(&cx).await.map_err(CrackedError::from)?;
    send_reply(&ctx, done.outcome().message(), true).await?;
    done.settle_now(&cx).await;
    Ok(())
}
