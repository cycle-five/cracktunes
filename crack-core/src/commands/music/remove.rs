use crate::{
    commands::cmd_check_music,
    errors::CrackedError,
    messaging::{courier, message::CrackedMessage},
    music::ops::{self, removed_embed, OpCx, Target},
    Context, Error,
};

/// Remove track(s) from the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Index in the queue to remove (Or number of tracks to remove if no second argument."]
    b_index: u32,
    #[description = "End index in the track queue to remove"] e_index: Option<u32>,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    remove_internal(ctx, b_index as usize, e_index.map(|i| i as usize)).await
}

/// Internal remove function.
pub async fn remove_internal(
    ctx: Context<'_>,
    b_index: usize,
    e_index: Option<usize>,
) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let target = match e_index {
        Some(end) => Target::Range(b_index, end),
        None => Target::Index(b_index),
    };
    let done = ops::remove(&cx, target).await.map_err(CrackedError::from)?;
    let removed = done.outcome();
    if removed.count == 1 {
        let embed = removed_embed(&removed.first, removed.thumbnail.as_deref());
        courier::reply(ctx, CrackedMessage::CreateEmbed(Box::new(embed))).await?;
    } else {
        courier::reply(ctx, CrackedMessage::RemoveMultiple).await?;
    }
    done.settle_now(&cx).await;
    Ok(())
}
