use crate::{
    handlers::track_end::update_queue_messages, messaging::message::CrackedMessage,
    utils::send_reply, Context, Error,
};
use rand::Rng;

/// Shuffle the current queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    handler.queue().modify_queue(|queue| {
        // skip the first track on queue because it's being played
        fisher_yates(
            queue.make_contiguous()[1..].as_mut(),
            &mut rand::thread_rng(),
        )
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
        values.swap(index, rng.gen_range(0..(index + 1)));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fisher_yates() {
        let mut values = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        fisher_yates(&mut values, &mut rand::thread_rng());
        assert_ne!(values, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
