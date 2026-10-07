use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    messaging::courier,
    music::ops::{self, OpCx},
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
    courier::reply(ctx, done.outcome().message()).await?;
    done.settle_now(&cx).await;
    Ok(())
}
