use crate::{
    commands::cmd_check_music, handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage, poise_ext::ContextExt, utils::send_reply, Context,
    CrackedError, Error,
};
use crack_types::errors::verify;
use rand::Rng;

/// Move a song in the queue to a different position.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn movesong(
    ctx: Context<'_>,
    #[description = "Index song is currently at"] at: usize,
    #[description = "Index song will be moved to"] to: usize,
) -> Result<(), Error> {
    movesong_internal(ctx, at, to).await
}

/// Move a song in the queue to a different position, internal function.
#[cfg(not(tarpaulin_include))]
pub async fn movesong_internal(ctx: Context<'_>, at: usize, to: usize) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let call = ctx.get_call().await?;

    let handler = call.lock().await;
    let len = handler.queue().current_queue().len();
    verify(
        at > 0 && at < len,
        CrackedError::Other("Index for `at` out of bounds"),
    )?;
    verify(
        to > 0 && to < len,
        CrackedError::Other("Index for `to` out of bounds"),
    )?;

    handler.queue().modify_queue(|queue| {
        // We verified before that this index is good, so this is safe.
        let song = queue.remove(at).expect("Index out of bounds");
        queue.insert(to, song);
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_reply(&ctx, CrackedMessage::SongMoved { at, to }, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

/// Shuffle the current queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let call = ctx.get_call().await?;

    let handler = call.lock().await;
    handler.queue().modify_queue(|queue| {
        // skip the first track on queue because it's being played
        fisher_yates(queue.make_contiguous()[1..].as_mut(), &mut rand::rng());
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_reply(&ctx, CrackedMessage::Shuffle, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

fn fisher_yates<T, R>(values: &mut [T], mut rng: R)
where
    R: rand::RngCore + Sized,
{
    let mut index = values.len();
    while index >= 2 {
        index -= 1;
        values.swap(index, rng.random_range(0..=index));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fisher_yates() {
        let mut values = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        fisher_yates(&mut values, &mut rand::rng());
        assert_ne!(values, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
