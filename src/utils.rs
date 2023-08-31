use self::serenity::{
    all::MessageInteraction, builder::CreateEmbed, http::Http, model::channel::Message,
};
use crate::{
    commands::{build_nav_btns, summon},
    errors::CrackedError,
    guild::settings::DEFAULT_LYRICS_PAGE_SIZE,
    messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS,
    Context, Data, Error,
};
use ::serenity::{
    all::CommandInteraction,
    builder::{
        CreateEmbedAuthor, CreateEmbedFooter, CreateInteractionResponse,
        CreateInteractionResponseMessage, EditInteractionResponse, EditMessage,
    },
    futures::StreamExt,
    prelude::Context as SerenityContext,
};
use poise::{
    serenity_prelude as serenity, CommandOrAutocompleteInteraction, CreateReply, FrameworkError,
    ReplyHandle,
};
use songbird::tracks::TrackHandle;
use std::{cmp::min, ops::Add, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use url::Url;

pub async fn create_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let e = CreateEmbed::default().description(format!("{message}"));
    // let reply = CreateReply::default().embed(embed);

    create_embed_response_poise(ctx, &e).await
}

pub async fn create_response_poise_text(
    ctx: &Context<'_>,
    message: CrackedMessage,
) -> Result<(), Error> {
    let message_str = format!("{message}");

    create_embed_response_str(ctx, message_str)
        .await
        .map(|_| Ok(()))?
}

pub async fn create_response(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    message: CrackedMessage,
) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    create_embed_response(http, interaction, embed).await
}

pub async fn create_response_text(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    content: &str,
) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(content);
    create_embed_response(http, interaction, embed).await
}

pub async fn edit_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default().description(format!("{message}"));

    match get_interaction(ctx) {
        Some(mut interaction) => {
            let res =
                edit_embed_response(&ctx.serenity_context().http, &mut interaction, embed).await;
            res.map(|_| ())
        }
        None => {
            create_embed_response_poise(ctx, &embed).await
            //return Err(Box::new(SerenityError::Other("No interaction found"))),
        }
    }
}

pub async fn edit_response(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    edit_embed_response(http, interaction, embed).await
}

pub async fn edit_response_text(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    content: &str,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(content);
    edit_embed_response(http, interaction, embed).await
}

pub async fn create_embed_response_str(
    ctx: &Context<'_>,
    message_str: String,
) -> Result<Message, Error> {
    let embed = CreateEmbed::new().description(message_str);
    ctx.send(poise::CreateReply::default().embed(embed))
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
}

pub async fn create_embed_response_poise(
    ctx: Context<'_>,
    embed: &CreateEmbed,
) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(mut interaction) => {
            create_embed_response(
                &ctx.serenity_context().http,
                &mut interaction,
                embed.clone(),
            )
            .await
        }
        None => create_embed_response_prefix(&ctx, embed.clone())
            .await
            .map(|_| Ok(()))?,
    }
}

pub async fn create_embed_response_prefix(
    ctx: &Context<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    ctx.send(CreateReply::default().embed(embed))
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
}

pub async fn create_embed_response(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    interaction
        .create_response(
            &http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::default().embed(embed),
            ),
        )
        .await
        .map_err(Into::into)
}

pub async fn edit_embed_response(
    http: &Arc<Http>,
    interaction: &mut CommandInteraction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    interaction
        .edit_response(
            &http,
            EditInteractionResponse::default()
                .content(" ")
                .add_embed(embed),
        )
        .await
        .map_err(Into::into)
}

pub enum ApplicationCommandOrMessageInteraction {
    ApplicationCommand(CommandInteraction),
    Message(MessageInteraction),
}

impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: MessageInteraction) -> Self {
        Self::Message(message)
    }
}

impl From<Box<MessageInteraction>> for ApplicationCommandOrMessageInteraction {
    fn from(message: Box<MessageInteraction>) -> Self {
        Self::Message(*message)
    }
}

impl From<CommandInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: CommandInteraction) -> Self {
        Self::ApplicationCommand(message)
    }
}

pub async fn edit_embed_response_poise(ctx: Context<'_>, embed: CreateEmbed) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_response(
                &ctx.serenity_context().http,
                EditInteractionResponse::default()
                    .content(" ")
                    .add_embed(embed),
            )
            .await
            .map(|_| Ok(()))?,
        None => create_embed_response_poise(ctx, &embed).await, //Err(Box::new(SerenityError::Other("No interaction found"))),
    }
}

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let metadata = track.metadata().clone();
    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    let channel = match metadata.channel.clone() {
        Some(channel) => format!(">>> {}", channel), //embed.field("Channel", format!(">>> {}", channel), true),
        None => ">>> N/A".to_owned(),                //embed.field("Channel", ">>> N/A", true),
    };

    let source_url = match metadata.source_url.clone() {
        Some(url) => {
            tracing::warn!("No source url found for track: {:?}", track);
            url
        }
        None => "".to_string(),
    };

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);
    let footer = CreateEmbedFooter::new(footer_text).icon_url(footer_icon_url);

    tracing::warn!("metadata: {:?}", metadata);

    let embed = CreateEmbed::default()
        .author(CreateEmbedAuthor::new(
            CrackedMessage::NowPlaying.to_string(),
        ))
        .title(metadata.title.unwrap_or_default())
        .url(source_url)
        .field("Progress", format!(">>> {} / {}", position, duration), true)
        .field("Channel", channel, true)
        .thumbnail(metadata.thumbnail.unwrap_or_default())
        .footer(footer);

    embed
}

pub async fn create_lyrics_embed_old(track: String, artists: String, lyric: String) -> CreateEmbed {
    //let mut embed = CreateEmbed::default();
    // let metadata = track_handle.metadata().clone();

    tracing::trace!("lyric: {}", lyric);
    tracing::trace!("track: {}", track);
    tracing::trace!("artists: {}", artists);

    let embed = CreateEmbed::default()
        .author(CreateEmbedAuthor::new(artists))
        .title(track)
        .description(lyric);

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

    embed
}

pub async fn create_lyrics_embed(
    ctx: Context<'_>,
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

pub async fn create_paged_embed(
    ctx: Context<'_>,
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
                CreateReply::new()
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

    while let Some(mci) = message
        .await_component_interactions(ctx)
        .timeout(Duration::from_secs(60 * 10))
        .next()
        .await
    {
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
                CreateInteractionResponseMessage::default()
                    .embed(
                        CreateEmbed::default()
                            .title(title.clone())
                            .author(CreateEmbedAuthor::new(author.clone()))
                            .description(page_getter(*page_wlock))
                            .footer(CreateEmbedFooter::new(format!(
                                "Page {}/{}",
                                *page_wlock + 1,
                                num_pages
                            ))),
                    )
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
        .await?;

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
                format!("{}:{:02}:{:02}", hours, minutes, seconds)
            }
        }
        None => "∞".to_string(),
    }
}

pub fn compare_domains(domain: &str, subdomain: &str) -> bool {
    subdomain == domain || subdomain.ends_with(domain)
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
pub fn check_msg(result: Result<Message, Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

use serenity::prelude::SerenityError;

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

pub fn get_interaction(ctx: Context<'_>) -> Option<CommandInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            CommandOrAutocompleteInteraction::Command(x) => Some(x.clone()),
            CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
    }
}

pub fn get_interaction_new(ctx: Context<'_>) -> Option<ApplicationCommandOrMessageInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            CommandOrAutocompleteInteraction::Command(x) => Some(x.clone().into()),
            CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(ctx) => ctx.msg.interaction.clone().map(|x| x.into()),
    }
}

pub fn get_user_id(ctx: &Context) -> serenity::UserId {
    match ctx {
        Context::Application(ctx) => match ctx.interaction {
            CommandOrAutocompleteInteraction::Command(x) => x.user.id,
            CommandOrAutocompleteInteraction::Autocomplete(x) => x.user.id,
        },
        Context::Prefix(ctx) => ctx.msg.author.id,
    }
}

pub fn get_channel_id(ctx: &Context) -> serenity::ChannelId {
    match ctx {
        Context::Application(ctx) => ctx.interaction.channel_id(),
        Context::Prefix(ctx) => ctx.msg.channel_id,
    }
}

pub async fn summon_short(ctx: Context<'_>) -> Result<(), FrameworkError<Data, Error>> {
    match ctx {
        Context::Application(ctx) => {
            tracing::warn!("summoning via slash command");
            summon().slash_action.unwrap()(ctx).await
        }
        Context::Prefix(ctx) => {
            tracing::warn!("summoning via prefix command");
            summon().prefix_action.unwrap()(ctx).await
        }
    }
}

pub async fn handle_error(
    ctx: &SerenityContext,
    interaction: &mut CommandInteraction,
    err: CrackedError,
) {
    create_response_text(&ctx.http, interaction, &format!("{err}"))
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
