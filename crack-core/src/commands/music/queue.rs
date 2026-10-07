use crate::{
    commands::cmd_check_music,
    errors::CrackedError,
    handlers::track_end::ModifyQueueHandler,
    messaging::{
        courier,
        interface::{create_nav_btns, create_queue_embed},
        message::CrackedMessage,
        messages::QUEUE_EXPIRED,
        render::Rendered,
        transport::DiscordTransport,
    },
    utils::{calculate_num_pages, forget_queue_message},
    Context, Error,
};
use ::serenity::builder::{CreateEmbed, CreateInteractionResponse};
use ::serenity::futures::StreamExt;
use songbird::{Event, TrackEvent};
use std::{cmp::min, ops::Add, sync::Arc, time::Duration};
use tokio::sync::RwLock;

const EMBED_TIMEOUT: u64 = 3600;

/// Display the current queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    aliases("list", "q"),
    guild_only
)]
pub async fn queue(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    queue_internal(ctx).await
}

use poise::serenity_prelude::CollectComponentInteractions;

/// Internal queue function.
#[cfg(not(tarpaulin_include))]
pub async fn queue_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = ctx.data().songbird.clone();
    let call = crate::commands::connected_call(&manager, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;

    // FIXME
    let handler = call.lock().await;
    let tracks = handler.queue().current_queue();
    drop(handler);
    tracing::info!("tracks: {:?}", tracks.len());

    let num_pages = calculate_num_pages(&tracks);
    tracing::info!("num_pages: {}", num_pages);

    let first = Rendered::embed(create_queue_embed(&tracks, 0).await)
        .with_components(create_nav_btns(0, num_pages));
    let message = courier::reply_rendered(ctx, first, false)
        .await?
        .into_message()
        .await?;

    let page: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

    // store this interaction to context.data for later edits
    let data = ctx.data();
    data.guild_cache_map
        .lock()
        .await
        .entry(guild_id)
        .or_default()
        .queue_messages
        .push((message.clone(), page.clone()));

    // refresh the queue interaction whenever a track ends
    crate::handlers::add_global_handler(
        &mut *call.lock().await,
        Event::Track(TrackEvent::End),
        ModifyQueueHandler {
            http: ctx.serenity_context().http.clone(),
            cache: ctx.serenity_context().cache.clone(),
            data: data.clone(),
            call: call.clone(),
            guild_id,
        },
    );

    let mut cib = message
        .id
        .collect_component_interactions(ctx.serenity_context())
        .timeout(Duration::from_secs(EMBED_TIMEOUT))
        .stream();

    while let Some(mci) = cib.next().await {
        let btn_id = &mci.data.custom_id;

        // refetch the queue in case it changed
        let tracks = call.lock().await.queue().current_queue();

        let page_num = {
            let mut page_wlock = page.write().await;

            *page_wlock = match btn_id.as_str() {
                "<<" => 0,
                "<" => min(page_wlock.saturating_sub(1), num_pages - 1),
                ">" => min(page_wlock.add(1), num_pages - 1),
                ">>" => num_pages - 1,
                _ => continue,
            };
            *page_wlock
        };

        let flipped = Rendered::embed(create_queue_embed(&tracks, page_num).await)
            .with_components(create_nav_btns(page_num, num_pages));
        mci.create_response(
            ctx.http(),
            CreateInteractionResponse::UpdateMessage(flipped.to_interaction_message()),
        )
        .await?;
    }

    let transport = DiscordTransport::of(ctx.serenity_context());
    let expired =
        CrackedMessage::CreateEmbed(Box::new(CreateEmbed::default().description(QUEUE_EXPIRED)));
    if let Err(err) =
        courier::edit_message(&transport, message.channel_id, message.id, &expired).await
    {
        tracing::warn!("could not mark the queue expired (message deleted?): {err:?}");
    }

    forget_queue_message(data, &message, guild_id)
        .await
        .map_err(Into::into)
}
