use self::serenity::{builder::CreateEmbed, http::Http, model::channel::Message};
use crate::{
    guild::settings::DEFAULT_LYRICS_PAGE_SIZE,
    messaging::{
        message::CrackedMessage,
        messages::{
            QUEUE_NOTHING_IS_PLAYING, QUEUE_NOW_PLAYING, QUEUE_NO_SONGS, QUEUE_NO_SRC,
            QUEUE_NO_TITLE, QUEUE_PAGE, QUEUE_PAGE_OF, QUEUE_UP_NEXT,
        },
    },
    metrics::COMMAND_EXECUTIONS,
    Context as CrackContext, CrackedError, Data, Error,
};
use ::serenity::{
    all::{ButtonStyle, GuildId, Interaction},
    builder::{
        CreateActionRow, CreateButton, CreateEmbedAuthor, CreateEmbedFooter,
        CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
        EditMessage,
    },
    futures::StreamExt,
};
use chrono;
use poise::{
    serenity_prelude::{
        self as serenity, CommandInteraction, Context as SerenityContext, CreateMessage,
        MessageInteraction,
    },
    CreateReply, ReplyHandle,
};
use songbird::{input::AuxMetadata, tracks::TrackHandle};
use std::fmt::Write;
use std::{
    cmp::{max, min},
    ops::Add,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use url::Url;
const EMBED_PAGE_SIZE: usize = 6;

/// Create and sends an log message as an embed.
/// FIXME: The avatar_url won't always be available. How do we best handle this?
pub async fn build_log_embed(
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<CreateEmbed, Error> {
    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let footer = CreateEmbedFooter::new(now_time_str);
    Ok(CreateEmbed::default()
        .title(title)
        .description(description)
        .thumbnail(avatar_url)
        .footer(footer))
}

/// Create and sends an log message as an embed.
/// FIXME: The avatar_url won't always be available. How do we best handle this?
pub async fn build_log_embed_thumb(
    title: &str,
    id: &str,
    description: &str,
    avatar_url: &str,
) -> Result<CreateEmbed, Error> {
    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let footer_str = format!("{} | {}", id, now_time_str);
    let footer = CreateEmbedFooter::new(footer_str);
    let author = CreateEmbedAuthor::new(title).icon_url(avatar_url);
    Ok(CreateEmbed::default()
        .author(author)
        // .title(title)
        .description(description)
        // .thumbnail(avatar_url)
        .footer(footer))
}

pub async fn send_log_embed_thumb(
    channel: &serenity::ChannelId,
    http: &Arc<Http>,
    id: &str,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, Error> {
    let embed = build_log_embed_thumb(title, id, description, avatar_url).await?;

    channel
        .send_message(http, CreateMessage::new().embed(embed))
        .await
        .map_err(Into::into)
}

pub async fn send_log_embed(
    channel: &serenity::ChannelId,
    http: &Arc<Http>,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, Error> {
    let embed = build_log_embed(title, description, avatar_url).await?;

    channel
        .send_message(http, CreateMessage::new().embed(embed))
        .await
        .map_err(Into::into)
}

/// Creates an embed from a CrackedMessage and sends it as an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_response_poise(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));

    send_embed_response_poise(ctx, embed).await
}

pub async fn send_response_poise_text(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let message_str = format!("{message}");

    send_embed_response_str(ctx, message_str).await
}

pub async fn create_response(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    send_embed_response(ctx, interaction, embed).await
}

pub async fn create_response_text(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(content);
    send_embed_response(ctx, interaction, embed).await
}

pub async fn edit_response_poise(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));

    match get_interaction_new(ctx) {
        Some(interaction) => {
            edit_embed_response(&ctx.serenity_context().http, &interaction, embed).await
        }
        None => send_embed_response_poise(ctx, embed).await,
    }
}

pub async fn edit_response(
    http: &Arc<Http>,
    interaction: &CommandOrMessageInteraction,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    edit_embed_response(http, interaction, embed).await
}

pub async fn edit_response_text(
    http: &Arc<Http>,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(content);
    edit_embed_response(http, interaction, embed).await
}

/// Sends a reply response as an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_str(
    ctx: CrackContext<'_>,
    message_str: String,
) -> Result<Message, Error> {
    ctx.send(
        CreateReply::default()
            .embed(CreateEmbed::new().description(message_str))
            .reply(true),
    )
    .await
    .unwrap()
    .into_message()
    .await
    .map_err(Into::into)
}

/// Sends a reply response with an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    tracing::warn!("create_embed_response_poise");
    ctx.send(
        CreateReply::default()
            .embed(embed)
            .ephemeral(false)
            .reply(true),
    )
    .await?
    .into_message()
    .await
    .map_err(|e| {
        tracing::error!("error: {:?}", e);
        e.into()
    })
}

pub async fn send_embed_response_prefix(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    ctx.send(CreateReply::default().embed(embed))
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
}

pub async fn send_embed_response(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    match interaction {
        CommandOrMessageInteraction::Command(int) => {
            tracing::warn!("CommandOrMessageInteraction::Command");
            create_response_interaction(&ctx.serenity_context().http, int, embed, false).await
        }
        CommandOrMessageInteraction::Message(_interaction) => {
            tracing::warn!("CommandOrMessageInteraction::Message");
            ctx.channel_id()
                .send_message(ctx.http(), CreateMessage::new().embed(embed))
                .await
                .map_err(Into::into)
        }
    }
}

pub async fn edit_reponse_interaction(
    http: &Arc<Http>,
    interaction: &Interaction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    match interaction {
        Interaction::Command(int) => int
            .edit_response(http, EditInteractionResponse::new().embed(embed.clone()))
            .await
            .map_err(Into::into),
        Interaction::Component(int) => int
            .edit_response(http, EditInteractionResponse::new().embed(embed.clone()))
            .await
            .map_err(Into::into),
        Interaction::Modal(int) => int
            .edit_response(http, EditInteractionResponse::new().embed(embed.clone()))
            .await
            .map_err(Into::into),
        Interaction::Autocomplete(int) => int
            .edit_response(http, EditInteractionResponse::new().embed(embed.clone()))
            .await
            //.map(|_| Message::default())
            .map_err(Into::into),
        Interaction::Ping(_int) => Ok(Message::default()),
        _ => todo!(),
    }
}

pub async fn create_response_interaction(
    http: &Arc<Http>,
    interaction: &Interaction,
    embed: CreateEmbed,
    _defer: bool,
) -> Result<Message, Error> {
    match interaction {
        Interaction::Command(int) => {
            // Is this "acknowledging" the interaction?
            // if defer {
            //     int.defer(http).await.unwrap();
            // }
            // let res = if defer {
            //     CreateInteractionResponse::Defer(
            //         CreateInteractionResponseMessage::new().embed(embed.clone()),
            //     )
            // } else {
            //     CreateInteractionResponse::Message(
            //         CreateInteractionResponseMessage::new().embed(embed.clone()),
            //     )
            // };

            let res = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(embed.clone()),
            );
            let message = int.get_response(http).await;
            match message {
                Ok(message) => {
                    message
                        .clone()
                        .edit(http, EditMessage::default().embed(embed.clone()))
                        .await?;
                    Ok(message)
                }
                Err(_) => {
                    int.create_response(http, res).await?;
                    let message = int.get_response(http).await?;
                    Ok(message)
                }
            }
        }
        Interaction::Ping(..)
        | Interaction::Component(..)
        | Interaction::Modal(..)
        | Interaction::Autocomplete(..) => Err(CrackedError::Other("not implemented").into()),
        _ => todo!(),
    }
}

pub async fn defer_response_interaction(
    http: &Arc<Http>,
    interaction: &Interaction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match interaction {
        Interaction::Command(int) => int
            .create_response(
                http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().embed(embed.clone()),
                ),
            )
            .await
            .map_err(Into::into),
        Interaction::Component(int) => int
            .create_response(
                http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().embed(embed.clone()),
                ),
            )
            .await
            .map_err(Into::into),
        Interaction::Modal(int) => int
            .create_response(
                http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().embed(embed.clone()),
                ),
            )
            .await
            .map_err(Into::into),
        Interaction::Autocomplete(int) => int
            .create_response(
                http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new().embed(embed.clone()),
                ),
            )
            .await
            .map_err(Into::into),
        Interaction::Ping(_int) => Ok(()),
        _ => todo!(),
    }
}

pub async fn edit_embed_response(
    http: &Arc<Http>,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    match interaction {
        CommandOrMessageInteraction::Command(int) => {
            edit_reponse_interaction(http, int, embed).await
        }
        CommandOrMessageInteraction::Message(msg) => match msg {
            Some(_msg) => {
                // Ok(CreateMessage::new().content("edit_embed_response not implemented").)
                Ok(Message::default())
                //    http.edit_origin, new_attachments)
                //     msg.user.id
            }
            _ => Ok(Message::default()),
        },
    }
}

pub enum ApplicationCommandOrMessageInteraction {
    Command(CommandInteraction),
    Message(MessageInteraction),
}

impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: MessageInteraction) -> Self {
        Self::Message(message)
    }
}

// impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
//     fn from(message: MessageInteraction) -> Self {
//         Self::ApplicationCommand(message)
//     }
// }

pub async fn edit_embed_response_poise(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    match get_interaction_new(ctx) {
        Some(interaction1) => match interaction1 {
            CommandOrMessageInteraction::Command(interaction2) => match interaction2 {
                Interaction::Command(interaction3) => {
                    tracing::warn!("CommandInteraction");
                    interaction3
                        .edit_response(
                            &ctx.serenity_context().http,
                            EditInteractionResponse::new().content(" ").embed(embed),
                        )
                        .await
                        .map_err(Into::into)
                }
                _ => Err(CrackedError::Other("not implemented").into()),
            },
            CommandOrMessageInteraction::Message(_) => send_embed_response_poise(ctx, embed).await,
        },
        None => send_embed_response_poise(ctx, embed).await,
    }
}

pub async fn get_track_metadata(track: &TrackHandle) -> AuxMetadata {
    let metadata = {
        let map = track.typemap().read().await;
        let my_metadata = match map.get::<crate::commands::MyAuxMetadata>() {
            Some(my_metadata) => my_metadata,
            None => {
                tracing::warn!("No metadata found for track: {:?}", track);
                return AuxMetadata::default();
            }
        };

        match my_metadata {
            crate::commands::MyAuxMetadata::Data(metadata) => metadata.clone(),
        }
    };
    metadata
}

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    // TrackHandle::metadata(track);
    let metadata = get_track_metadata(track).await;

    tracing::warn!("metadata: {:?}", metadata);

    let title = metadata.title.clone().unwrap_or_default();

    let source_url = metadata.source_url.clone().unwrap_or_default();

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    let progress_field = ("Progress", format!(">>> {} / {}", position, duration), true);

    let channel_field: (&'static str, String, bool) = match metadata.channel.clone() {
        Some(channel) => ("Channel", format!(">>> {}", channel), true),
        None => ("Channel", ">>> N/A".to_string(), true),
    };

    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);

    CreateEmbed::new()
        .author(CreateEmbedAuthor::new(CrackedMessage::NowPlaying))
        .title(title.clone())
        .url(source_url)
        .field(progress_field.0, progress_field.1, progress_field.2)
        .field(channel_field.0, channel_field.1, channel_field.2)
        // .thumbnail(url::Url::parse(&thumbnail).unwrap())
        .thumbnail(
            url::Url::parse(&thumbnail)
                .map(|x| x.to_string())
                .map_err(|e| {
                    tracing::error!("error parsing url: {:?}", e);
                    "".to_string()
                })
                .unwrap_or_default(),
        )
        .footer(CreateEmbedFooter::new(footer_text).icon_url(footer_icon_url))
}

pub async fn create_lyrics_embed_old(track: String, artists: String, lyric: String) -> CreateEmbed {
    // let metadata = track_handle.metadata().clone();

    tracing::trace!("lyric: {}", lyric);
    tracing::trace!("track: {}", track);
    tracing::trace!("artists: {}", artists);

    // embed.author(|author| author.name(artists));
    // embed.title(track);
    // embed.description(lyric);
    CreateEmbed::default()
        .author(CreateEmbedAuthor::new(artists))
        .title(track)
        .description(lyric)

    // metadata
    //     .source_url
    //     .as_ref()
    //     .map(|source_url| embed.url(source_url.clone()));

    // metadata
    //     .thumbnail
    //     .as_ref()
    //     .map(|thumbnail| embed.thumbnail(thumbnail));

    // let source_url = metadata.source_url.unwrap_or_else(|| {
    //     tracing::warn!("No source url found for track: {:?}", track);
    //     "".to_string()
    // });

    // let (footer_text, footer_icon_url) = get_footer_info(&source_url);
    // embed.footer(|f| f.text(footer_text).icon_url(footer_icon_url));
}

pub async fn create_search_results_reply(
    // ctx: CrackContext<'_>,
    // results: Vec<String>,
    results: Vec<CreateEmbed>,
) -> CreateReply {
    let mut reply = CreateReply::default()
        .reply(true)
        .content("Search results:");
    for result in results {
        reply.embeds.push(result);
    }

    reply.clone()

    // let mut embed = CreateEmbed::default();
    // for (i, result) in results.iter().enumerate() {
    //     let _ = embed.field(
    //         format!("{}. {}", i + 1, result),
    //         format!("[{}]({})", result, result),
    //         false,
    //     );
    // }

    // CreateReply::default()
    //     .
    //     .embeds(embed.clone())
    //     .content("Search results:")
    //     .reply(true)
}

pub async fn create_lyrics_embed(
    ctx: CrackContext<'_>,
    track: String,
    artists: String,
    lyric: String,
) -> Result<(), Error> {
    create_paged_embed(
        ctx,
        artists,
        track,
        lyric,
        DEFAULT_LYRICS_PAGE_SIZE, //ctx.data().bot_settings.lyrics_page_size,
    )
    .await
}

fn build_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton {
    CreateButton::new(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

pub fn build_nav_btns(page: usize, num_pages: usize) -> Vec<CreateActionRow> {
    let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);
    vec![CreateActionRow::Buttons(vec![
        build_single_nav_btn("<<", cant_left),
        build_single_nav_btn("<", cant_left),
        build_single_nav_btn(">", cant_right),
        build_single_nav_btn(">>", cant_right),
    ])]
}

#[allow(dead_code)]
async fn build_queue_page(tracks: &[TrackHandle], page: usize) -> String {
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

    for (i, &t) in queue.iter().enumerate() {
        let metadata = get_track_metadata(t).await;
        let title = metadata.title.as_ref().unwrap();
        let url = metadata.source_url.as_ref().unwrap();
        let duration = get_human_readable_timestamp(metadata.duration);

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
    message: &Message,
    guild_id: GuildId,
) -> Result<(), CrackedError> {
    let mut cache_map = data.guild_cache_map.lock().unwrap().clone();

    let cache = cache_map
        .get_mut(&guild_id)
        .ok_or(CrackedError::NoGuildId)?;
    cache.queue_messages.retain(|(m, _)| m.id != message.id);

    Ok(())
}

pub async fn create_queue_embed(tracks: &[TrackHandle], page: usize) -> CreateEmbed {
    let (description, thumbnail) = if !tracks.is_empty() {
        let metadata = get_track_metadata(&tracks[0]).await;
        let thumbnail = metadata.thumbnail.unwrap_or_default();

        let description = format!(
            "[{}]({}) • `{}`",
            metadata
                .title
                .as_ref()
                .unwrap_or(&String::from(QUEUE_NO_TITLE)),
            metadata
                .source_url
                .as_ref()
                .unwrap_or(&String::from(QUEUE_NO_SRC)),
            get_human_readable_timestamp(metadata.duration)
        );
        (description, thumbnail)
    } else {
        (String::from(QUEUE_NOTHING_IS_PLAYING), "".to_string())
    };

    CreateEmbed::default()
        .thumbnail(thumbnail)
        .field(QUEUE_NOW_PLAYING, &description, false)
        .field(QUEUE_UP_NEXT, build_queue_page(tracks, page).await, false)
        .footer(CreateEmbedFooter::new(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(tracks),
        )))
}

pub async fn create_paged_embed(
    ctx: CrackContext<'_>,
    author: String,
    title: String,
    content: String,
    page_size: usize,
) -> Result<(), Error> {
    // let mut embed = CreateEmbed::default();
    let page_getter = create_lyric_page_getter(&content, page_size);
    let num_pages = content.len() / page_size + 1;
    let page: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

    let mut message = {
        let reply = ctx
            .send(
                CreateReply::default()
                    .embed(
                        CreateEmbed::new()
                            .title(title.clone())
                            .author(CreateEmbedAuthor::new(author.clone()))
                            .description(page_getter(0))
                            .footer(CreateEmbedFooter::new(format!("Page {}/{}", 1, num_pages))),
                    )
                    .components(build_nav_btns(0, num_pages)),
            )
            .await?;
        reply.into_message().await?
    };

    let mut cib = message
        .await_component_interactions(ctx)
        .timeout(Duration::from_secs(60 * 10))
        .stream();

    while let Some(mci) = cib.next().await {
        let btn_id = &mci.data.custom_id;

        let mut page_wlock = page.write().await;

        *page_wlock = match btn_id.as_str() {
            "<<" => 0,
            "<" => min(page_wlock.saturating_sub(1), num_pages - 1),
            ">" => min(page_wlock.add(1), num_pages - 1),
            ">>" => num_pages - 1,
            _ => continue,
        };

        mci.create_response(
            &ctx,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embeds(vec![CreateEmbed::new()
                        .title(title.clone())
                        .author(CreateEmbedAuthor::new(author.clone()))
                        .description(page_getter(*page_wlock))
                        .footer(CreateEmbedFooter::new(format!(
                            "Page {}/{}",
                            *page_wlock + 1,
                            num_pages
                        )))])
                    .components(build_nav_btns(*page_wlock, num_pages)),
            ),
        )
        .await?;
    }

    message
        .edit(
            &ctx.serenity_context().http,
            EditMessage::default().embed(
                CreateEmbed::default()
                    .description("Lryics timed out, run the command again to see them."),
            ),
        )
        .await
        .unwrap();

    Ok(())
}

pub fn split_string_into_chunks(string: &str, chunk_size: usize) -> Vec<String> {
    string
        .chars()
        .collect::<Vec<char>>()
        .chunks(chunk_size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// Splits lyrics into chunks of a given size, but tries to split on a newline if possible.
pub fn split_lyric_string_into_chunks(string: &str, chunk_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let end = string.len();
    let mut cur: usize = 0;
    while cur < end {
        let mut next = min(cur + chunk_size, end);
        let chunk = &string[cur..next];
        let newline_index = chunk.rfind('\n');
        let chunk = match newline_index {
            Some(index) => {
                next = index + cur + 1;
                &chunk[..index]
            }
            None => chunk,
        };
        chunks.push(chunk.to_string());
        cur = next;
    }

    chunks
}

pub fn create_page_getter(string: &str, chunk_size: usize) -> impl Fn(usize) -> String {
    let chunks = split_string_into_chunks(string, chunk_size);
    move |page| {
        let page = page % chunks.len();
        chunks[page].clone()
    }
}

pub fn create_lyric_page_getter(string: &str, chunk_size: usize) -> impl Fn(usize) -> String + '_ {
    let chunks = split_lyric_string_into_chunks(string, chunk_size);
    move |page| {
        let page = page % chunks.len();
        chunks[page].clone()
    }
}

pub fn get_footer_info(url: &str) -> (String, String) {
    let url_data = match Url::parse(url) {
        Ok(url_data) => url_data,
        Err(_) => {
            return (
                "Streaming via unknown".to_string(),
                "https://www.google.com/s2/favicons?domain=unknown".to_string(),
            )
        }
    };
    let domain = url_data.host_str().unwrap();

    // remove www prefix because it looks ugly
    let domain = domain.replace("www.", "");

    (
        format!("Streaming via {}", domain),
        format!("https://www.google.com/s2/favicons?domain={}", domain),
    )
}

pub fn get_human_readable_timestamp(duration: Option<Duration>) -> String {
    match duration {
        Some(duration) if duration == Duration::MAX => "∞".to_string(),
        Some(duration) => {
            let seconds = duration.as_secs() % 60;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = duration.as_secs() / 3600;

            if hours < 1 {
                format!("{:02}:{:02}", minutes, seconds)
            } else {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        }
        None => "∞".to_string(),
    }
}

use serenity::prelude::SerenityError;

pub fn compare_domains(domain: &str, subdomain: &str) -> bool {
    subdomain == domain || subdomain.ends_with(domain)
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
pub fn check_msg(result: Result<Message, Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub fn check_reply(result: Result<ReplyHandle, SerenityError>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub fn check_interaction(result: Result<(), Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub enum CommandOrMessageInteraction {
    Command(Interaction),
    Message(Option<Box<MessageInteraction>>),
}

pub fn get_interaction(ctx: CrackContext<'_>) -> Option<CommandInteraction> {
    match ctx {
        CrackContext::Application(app_ctx) => app_ctx.interaction.clone().into(),
        // match app_ctx.interaction {
        //     CommandOrAutocompleteInteraction::Command(x) => Some(x.clone()),
        //     CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        // },
        // Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
        CrackContext::Prefix(_ctx) => None,
    }
}

pub fn get_interaction_new(ctx: CrackContext<'_>) -> Option<CommandOrMessageInteraction> {
    match ctx {
        CrackContext::Application(app_ctx) => {
            Some(CommandOrMessageInteraction::Command(Interaction::Command(
                app_ctx.interaction.clone(),
            )))
            // match app_ctx.interaction {
            // CommandOrAutocompleteInteraction::Command(x) => Some(
            //     CommandOrMessageInteraction::Command(Interaction::Command(x.clone())),
            // ),
            // CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        }
        // Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
        CrackContext::Prefix(ctx) => Some(CommandOrMessageInteraction::Message(
            ctx.msg.interaction.clone(),
        )),
    }
}

// pub fn get_interaction_new(ctx: Context<'_>) -> Option<ApplicationCommandOrMessageInteraction> {
//     match ctx {
//         Context::Application(app_ctx) => match app_ctx.interaction {
//             ApplicationCommandOrMessageInteraction::ApplicationCommand(x) => Some(x.clone().into()),
//             ApplicationCommandOrMessageInteraction::Autocomplete(_) => None,
//         },
//         Context::Prefix(ctx) => ctx.msg.interaction.clone().map(|x| x.into()),
//     }
// }

pub fn get_user_id(ctx: &CrackContext) -> serenity::UserId {
    match ctx {
        CrackContext::Application(ctx) => ctx.interaction.user.id,
        CrackContext::Prefix(ctx) => ctx.msg.author.id,
    }
}

pub fn get_channel_id(ctx: &CrackContext) -> serenity::ChannelId {
    match ctx {
        CrackContext::Application(ctx) => ctx.interaction.channel_id,
        CrackContext::Prefix(ctx) => ctx.msg.channel_id,
    }
}

// pub async fn summon_short(ctx: CrackContext<'_>) -> Result<(), FrameworkError<Data, Error>> {
//     match ctx {
//         CrackContext::Application(_ctx) => {
//             tracing::warn!("summoning via slash command");
//             // summon().slash_action.unwrap()(ctx).await
//             // FIXME
//             Ok(())
//         }
//         CrackContext::Prefix(_ctx) => {
//             tracing::warn!("summoning via prefix command");
//             // summon().prefix_action.unwrap()(ctx).await
//             // FIXME
//             Ok(())
//         }
//     }
// }

pub async fn handle_error(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    err: CrackedError,
) {
    create_response_text(ctx, interaction, &format!("{err}"))
        .await
        .expect("failed to create response");
}
pub fn count_command(command: &str, is_prefix: bool) {
    tracing::warn!("counting command: {}", command);
    match COMMAND_EXECUTIONS
        .get_metric_with_label_values(&[command, if is_prefix { "prefix" } else { "slash" }])
    {
        Ok(metric) => {
            metric.inc();
        }
        Err(e) => {
            tracing::error!("Failed to get metric: {}", e);
        }
    };
}

/// Gets the channel id that the bot is currently playing in for a given guild.
pub async fn get_current_voice_channel_id(
    ctx: &SerenityContext,
    guild_id: serenity::GuildId,
) -> Option<serenity::ChannelId> {
    let manager = songbird::get(ctx)
        .await
        .expect("Failed to get songbird manager")
        .clone();

    let call_lock = manager.get(guild_id)?;
    let call = call_lock.lock().await;

    let channel_id = call.current_channel()?;
    let serenity_channel_id = serenity::ChannelId::new(channel_id.0.into());

    Some(serenity_channel_id)
}

pub fn get_guild_name(ctx: &SerenityContext, guild_id: serenity::GuildId) -> Option<String> {
    let guild = ctx.cache.guild(guild_id)?;
    Some(guild.name.clone())
}

#[cfg(test)]
mod tests {

    use ::serenity::all::Button;

    use super::*;

    #[test]
    fn test_get_human_readable_timestamp() {
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::new(3661, 0))),
            "01:01:01"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::new(59, 0))),
            "00:59"
        );
        assert_eq!(get_human_readable_timestamp(None), "∞");
    }

    #[test]
    fn test_compare_domains() {
        assert!(compare_domains("example.com", "example.com"));
        assert!(compare_domains("example.com", "sub.example.com"));
        assert!(!compare_domains("example.com", "example.org"));
        assert!(compare_domains("example.com", "anotherexample.com"));
    }

    #[test]
    fn test_get_footer_info() {
        let (text, icon_url) = get_footer_info("https://www.rust-lang.org/");
        assert_eq!(text, "Streaming via rust-lang.org");
        assert!(icon_url.contains("rust-lang.org"));
    }

    #[test]
    fn test_build_single_nav_btn() {
        let creat_btn = build_single_nav_btn("<<", true);
        let s = serde_json::to_string_pretty(&creat_btn).unwrap();
        println!("s: {}", s);
        let btn = serde_json::from_str::<Button>(&s).unwrap();

        assert_eq!(btn.label, Some("<<".to_string()));
        // assert_eq!(btn.style, ButtonStyle::Primary);
        assert_eq!(btn.disabled, true);
    }
}
