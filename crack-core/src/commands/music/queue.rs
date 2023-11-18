use self::serenity::{builder::CreateEmbed, futures::StreamExt};
use crate::{
    handlers::track_end::ModifyQueueHandler,
    messaging::messages::QUEUE_EXPIRED,
    utils::{
        build_nav_btns, calculate_num_pages, create_queue_embed, forget_queue_message,
        get_interaction,
    },
    Context, Error,
};
use ::serenity::builder::{
    CreateInteractionResponse, CreateInteractionResponseMessage, EditMessage,
};
use poise::{
    serenity_prelude::{self as serenity},
    CreateReply,
};
use songbird::{Event, TrackEvent};
use std::{cmp::min, ops::Add, sync::Arc, sync::RwLock, time::Duration};

const EMBED_TIMEOUT: u64 = 3600;

/// Display the current queue.
#[poise::command(slash_command, prefix_command, aliases("list", "q"), guild_only)]
pub async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    tracing::info!("queue called");
    let guild_id = ctx.guild_id().unwrap();
    tracing::info!("guild_id: {}", guild_id);
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    tracing::info!("manager: {:?}", manager);
    //let call = manager.get_or_insert(guild_id); //.unwrap();
    let call = manager.get(guild_id).unwrap();
    // let call = match manager.get(guild_id) {
    //     Some(call) => call,
    //     None => {
    //         let embed =
    //             CreateEmbed::default().description(format!("{}", CrackedError::NotConnected));
    //         send_embed_response_poise(ctx, embed).await?;
    //         return Ok(());
    //     }
    // };

    let handler = call.lock().await;
    let tracks = handler.queue().current_queue();
    drop(handler);
    tracing::info!("tracks: {:?}", tracks);

    let num_pages = calculate_num_pages(&tracks);
    tracing::info!("num_pages: {}", num_pages);

    let mut message = match get_interaction(ctx) {
        Some(interaction) => {
            interaction
                .create_response(
                    &ctx.serenity_context().http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .embed(create_queue_embed(&tracks, 0).await)
                            .components(build_nav_btns(0, num_pages)),
                    ),
                )
                .await?;
            interaction
                .get_response(&ctx.serenity_context().http)
                .await?
        }
        _ => {
            let reply = ctx
                .send(
                    CreateReply::new()
                        .embed(create_queue_embed(&tracks, 0).await)
                        .components(build_nav_btns(0, num_pages)),
                )
                .await?;
            reply.into_message().await?
        }
    };

    let page: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

    // store this interaction to context.data for later edits
    let data = ctx.data().clone();
    let mut cache_map = data.guild_cache_map.lock().unwrap().clone();

    let cache = cache_map.entry(guild_id).or_default();
    cache.queue_messages.push((message.clone(), page.clone()));
    drop(data);

    // refresh the queue interaction whenever a track ends
    let mut handler = call.lock().await;
    // let data = ctx.data().clone();
    handler.add_global_event(
        Event::Track(TrackEvent::End),
        ModifyQueueHandler {
            http: ctx.serenity_context().http.clone(),
            data: ctx.data().clone(),
            call: call.clone(),
            guild_id,
        },
    );
    drop(handler);

    let mut cib = message
        .await_component_interactions(ctx)
        .timeout(Duration::from_secs(EMBED_TIMEOUT))
        .stream();

    while let Some(mci) = cib.next().await {
        let btn_id = &mci.data.custom_id;

        // refetch the queue in case it changed
        let handler = call.lock().await;
        let tracks = handler.queue().current_queue();
        drop(handler);

        let page_num = {
            let mut page_wlock = page.write().unwrap();

            *page_wlock = match btn_id.as_str() {
                "<<" => 0,
                "<" => min(page_wlock.saturating_sub(1), num_pages - 1),
                ">" => min(page_wlock.add(1), num_pages - 1),
                ">>" => num_pages - 1,
                _ => continue,
            };
            *page_wlock
        };

        mci.create_response(
            &ctx,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .add_embed(create_queue_embed(&tracks, page_num).await)
                    .components(build_nav_btns(page_num, num_pages)),
            ),
        )
        .await?;
    }

    message
        .edit(
            &ctx.serenity_context().http,
            EditMessage::new().embed(CreateEmbed::default().description(QUEUE_EXPIRED)),
        )
        .await
        .unwrap();

    forget_queue_message(ctx.data(), &message, guild_id)
        .await
        .ok();

    Ok(())
}
