use self::serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use crate::{
    handlers::track_end::update_queue_messages,
    messaging::message::ParrotMessage,
    utils::{create_response, get_interaction},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use rand::Rng;

#[poise::command(prefix_command, slash_command)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    let mut interaction = get_interaction(ctx).unwrap();
    let guild_id = interaction.guild_id.unwrap();
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

    create_response(
        &ctx.serenity_context().http,
        &mut interaction,
        ParrotMessage::Shuffle,
    )
    .await?;
    update_queue_messages(
        &ctx.serenity_context().http,
        &ctx.serenity_context().data,
        &queue,
        guild_id,
    )
    .await;
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
