use self::serenity::builder::CreateEmbed;
use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    messaging::messages::REMOVED_QUEUE,
    utils::send_response_poise_text,
    utils::{get_track_metadata, send_embed_response_poise},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;
use std::cmp::min;

/// Remove track(s) from the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Start index in the track queue to remove"] b_index: usize,
    #[description = "End index in the track queue to remove"] e_index: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let remove_index = b_index;
    let remove_until = match e_index {
        Some(arg) => arg,
        None => remove_index,
    };

    let handler = call.lock().await;
    let queue = handler.queue().current_queue();

    let queue_len = queue.len();
    let remove_until = min(remove_until, queue_len.saturating_sub(1));

    verify(queue_len > 1, CrackedError::QueueEmpty)?;
    verify(
        remove_index < queue_len,
        CrackedError::NotInRange("index", remove_index as isize, 1, queue_len as isize),
    )?;
    verify(
        remove_until >= remove_index,
        CrackedError::NotInRange(
            "until",
            remove_until as isize,
            remove_index as isize,
            queue_len as isize,
        ),
    )?;

    let track = queue.get(remove_index).unwrap();

    handler.queue().modify_queue(|v| {
        v.drain(remove_index..=remove_until);
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    if remove_until == remove_index {
        let embed = create_remove_enqueued_embed(track).await;
        //send_embed_response(&ctx.serenity_context().http, interaction, embed).await?;
        send_embed_response_poise(ctx, embed).await?;
    } else {
        send_response_poise_text(ctx, CrackedMessage::RemoveMultiple).await?;
    }

    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

async fn create_remove_enqueued_embed(track: &TrackHandle) -> CreateEmbed {
    let metadata = get_track_metadata(track).await;
    CreateEmbed::default()
        .field(
            REMOVED_QUEUE,
            format!(
                "[**{}**]({})",
                metadata.title.unwrap(),
                metadata.source_url.unwrap()
            ),
            false,
        )
        .thumbnail(metadata.thumbnail.unwrap())
}
