use self::serenity::builder::CreateEmbed;
use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    messaging::messages::REMOVED_QUEUE,
    utils::send_reply,
    utils::{get_track_handle_metadata, send_embed_response_poise},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;
use std::cmp::min;

/// Remove track(s) from the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Music", prefix_command, slash_command, guild_only)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Index in the queue to remove (Or number of tracks to remove if no second argument."]
    b_index: usize,
    #[description = "End index in the track queue to remove"] e_index: Option<usize>,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    remove_internal(ctx, b_index, e_index).await
}

/// Internal remove function.
pub async fn remove_internal(
    ctx: Context<'_>,
    b_index: usize,
    e_index: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = ctx.data().songbird.clone();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

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
        // This is what songbird does internally when it stops the queue
        // so it should be the right thing to do here.
        v.drain(remove_index..=remove_until).for_each(|x| {
            let _ = x.stop();
            drop(x);
        });
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    if remove_until == remove_index {
        let embed = create_remove_enqueued_embed(track).await;
        //send_embed_response(&ctx.serenity_context().http, interaction, embed).await?;
        send_embed_response_poise(&ctx, embed).await?;
    } else {
        send_reply(&ctx, CrackedMessage::RemoveMultiple, true).await?;
    }

    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

async fn create_remove_enqueued_embed(track: &TrackHandle) -> CreateEmbed {
    let metadata = get_track_handle_metadata(track).await;
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
