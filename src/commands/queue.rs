use self::serenity::{
    builder::{CreateButton, CreateComponents, CreateEmbed},
    futures::StreamExt,
    model::{
        application::{component::ButtonStyle, interaction::InteractionResponseType},
        channel::Message,
        id::GuildId,
    },
    RwLock,
};
use crate::{
    errors::CrackedError,
    handlers::track_end::ModifyQueueHandler,
    messaging::messages::{
        QUEUE_EXPIRED, QUEUE_NOTHING_IS_PLAYING, QUEUE_NOW_PLAYING, QUEUE_NO_SONGS, QUEUE_PAGE,
        QUEUE_PAGE_OF, QUEUE_UP_NEXT,
    },
    utils::{create_embed_response_poise, get_human_readable_timestamp, get_interaction},
    Context, Data, Error,
};
use poise::serenity_prelude::{self as serenity};
use songbird::{tracks::TrackHandle, Event, TrackEvent};
use std::{
    cmp::{max, min},
    fmt::Write,
    ops::Add,
    sync::Arc,
    time::Duration,
};

const EMBED_PAGE_SIZE: usize = 6;
const EMBED_TIMEOUT: u64 = 3600;

/// Display the current queue.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    //COMMAND_EXECUTIONS.with_label_values(&["queue"]).inc();
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let handler = call.lock().await;
    let tracks = handler.queue().current_queue();
    drop(handler);

    tracing::trace!("tracks: {:?}", tracks);

    let mut message = match get_interaction(ctx) {
        Some(interaction) => {
            interaction
                .create_interaction_response(&ctx.serenity_context().http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            let num_pages = calculate_num_pages(&tracks);

                            message
                                .add_embed(create_queue_embed(&tracks, 0))
                                .components(|components| build_nav_btns(components, 0, num_pages))
                        })
                })
                .await?;
            interaction
                .get_interaction_response(&ctx.serenity_context().http)
                .await?
        }
        _ => {
            let reply = ctx
                .send(|m| {
                    let num_pages = calculate_num_pages(&tracks);
                    m.embeds.push(create_queue_embed(&tracks, 0));
                    m.components(|components| build_nav_btns(components, 0, num_pages))
                })
                .await?;
            reply.into_message().await?
        }
    };

    // let mut message = interaction
    //     .get_interaction_response(&ctx.serenity_context().http)
    //     .await?;
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
        .build();

    while let Some(mci) = cib.next().await {
        let btn_id = &mci.data.custom_id;

        // refetch the queue in case it changed
        let handler = call.lock().await;
        let tracks = handler.queue().current_queue();
        drop(handler);

        let num_pages = calculate_num_pages(&tracks);
        let mut page_wlock = page.write().await;

        *page_wlock = match btn_id.as_str() {
            "<<" => 0,
            "<" => min(page_wlock.saturating_sub(1), num_pages - 1),
            ">" => min(page_wlock.add(1), num_pages - 1),
            ">>" => num_pages - 1,
            _ => continue,
        };

        mci.create_interaction_response(&ctx, |r| {
            r.kind(InteractionResponseType::UpdateMessage);
            r.interaction_response_data(|d| {
                d.add_embed(create_queue_embed(&tracks, *page_wlock));
                d.components(|components| build_nav_btns(components, *page_wlock, num_pages))
            })
        })
        .await?;
    }

    message
        .edit(&ctx.serenity_context().http, |edit| {
            let mut embed = CreateEmbed::default();
            embed.description(QUEUE_EXPIRED);
            edit.set_embed(embed);
            edit.components(|f| f)
        })
        .await
        .unwrap();

    forget_queue_message(ctx.data(), &mut message, guild_id)
        .await
        .ok();

    Ok(())
}

pub fn create_queue_embed(tracks: &[TrackHandle], page: usize) -> CreateEmbed {
    let mut embed: CreateEmbed = CreateEmbed::default();

    let description = if !tracks.is_empty() {
        let metadata = tracks[0].metadata();
        embed.thumbnail(tracks[0].metadata().thumbnail.as_ref().unwrap());

        format!(
            "[{}]({}) • `{}`",
            metadata.title.as_ref().unwrap(),
            metadata.source_url.as_ref().unwrap(),
            get_human_readable_timestamp(metadata.duration)
        )
    } else {
        String::from(QUEUE_NOTHING_IS_PLAYING)
    };

    embed.field(QUEUE_NOW_PLAYING, &description, false);
    embed.field(QUEUE_UP_NEXT, &build_queue_page(tracks, page), false);

    embed.footer(|f| {
        f.text(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(tracks),
        ))
    });

    embed
}

fn build_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton {
    CreateButton::default()
        .custom_id(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

pub fn build_nav_btns(
    components: &mut CreateComponents,
    page: usize,
    num_pages: usize,
) -> &mut CreateComponents {
    components.create_action_row(|action_row| {
        let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);

        action_row
            .add_button(build_single_nav_btn("<<", cant_left))
            .add_button(build_single_nav_btn("<", cant_left))
            .add_button(build_single_nav_btn(">", cant_right))
            .add_button(build_single_nav_btn(">>", cant_right))
    })
}

fn build_queue_page(tracks: &[TrackHandle], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    let queue: Vec<&TrackHandle> = tracks
        .iter()
        .skip(start_idx + 1)
        .take(EMBED_PAGE_SIZE)
        .collect();

    if queue.is_empty() {
        return String::from(QUEUE_NO_SONGS);
    }

    let mut description = String::new();

    for (i, t) in queue.iter().enumerate() {
        let title = t.metadata().title.as_ref().unwrap();
        let url = t.metadata().source_url.as_ref().unwrap();
        let duration = get_human_readable_timestamp(t.metadata().duration);

        let _ = writeln!(
            description,
            "`{}.` [{}]({}) • `{}`",
            i + start_idx + 1,
            title,
            url,
            duration
        );
    }

    description
}

pub fn calculate_num_pages(tracks: &[TrackHandle]) -> usize {
    let num_pages = ((tracks.len() as f64 - 1.0) / EMBED_PAGE_SIZE as f64).ceil() as usize;
    max(1, num_pages)
}

pub async fn forget_queue_message(
    data: &Data,
    message: &mut Message,
    guild_id: GuildId,
) -> Result<(), ()> {
    let mut cache_map = data.guild_cache_map.lock().unwrap().clone();

    let cache = cache_map.get_mut(&guild_id).ok_or(())?;
    cache.queue_messages.retain(|(m, _)| m.id != message.id);

    Ok(())
}
