use self::serenity::{
    builder::CreateEmbed,
    http::Http,
    model::{
        application::interaction::{
            application_command::ApplicationCommandInteraction, InteractionResponseType,
            MessageInteraction,
        },
        channel::Message,
    },
    Context as SerenityContext, SerenityError,
};
use crate::{
    commands::{build_nav_btns, summon},
    errors::CrackedError,
    guild::settings::DEFAULT_LYRICS_PAGE_SIZE,
    messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS,
    Context, Data, Error,
};
use ::serenity::futures::StreamExt;
use poise::{
    serenity_prelude as serenity, ApplicationCommandOrAutocompleteInteraction, FrameworkError,
    ReplyHandle,
};
use songbird::tracks::TrackHandle;
use std::{cmp::min, ops::Add, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use url::Url;

pub async fn create_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    create_embed_response_poise(ctx, embed).await
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
    interaction: &ApplicationCommandInteraction,
    message: CrackedMessage,
) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));
    create_embed_response(http, interaction, embed).await
}

pub async fn create_response_text(
    http: &Arc<Http>,
    interaction: &ApplicationCommandInteraction,
    content: &str,
) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(content);
    create_embed_response(http, interaction, embed).await
}

pub async fn edit_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    match get_interaction(ctx) {
        Some(interaction) => {
            let res = edit_embed_response(&ctx.serenity_context().http, &interaction, embed).await;
            res.map(|_| ())
        }
        None => {
            create_embed_response_poise(ctx, embed).await
            //return Err(Box::new(SerenityError::Other("No interaction found"))),
        }
    }
}

pub async fn edit_response(
    http: &Arc<Http>,
    interaction: &ApplicationCommandInteraction,
    message: CrackedMessage,
) -> Result<Message, Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));
    edit_embed_response(http, interaction, embed).await
}

pub async fn edit_response_text(
    http: &Arc<Http>,
    interaction: &ApplicationCommandInteraction,
    content: &str,
) -> Result<Message, Error> {
    let mut embed = CreateEmbed::default();
    embed.description(content);
    edit_embed_response(http, interaction, embed).await
}

pub async fn create_embed_response_str(
    ctx: &Context<'_>,
    message_str: String,
) -> Result<Message, Error> {
    ctx.send(|b| b.embed(|e| e.description(message_str)).reply(true))
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
    //.map_err(Into::into)
    // Ok(())
}

pub async fn create_embed_response_poise(
    ctx: Context<'_>,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(interaction) => {
            create_embed_response(&ctx.serenity_context().http, &interaction, embed).await
        }
        None => create_embed_response_prefix(&ctx, embed)
            .await
            .map(|_| Ok(()))?,
    }
}

pub async fn create_embed_response_prefix(
    ctx: &Context<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    ctx.send(|builder| {
        builder.embeds.append(&mut vec![embed]);
        builder //.reply(true)
    })
    .await
    .unwrap()
    .into_message()
    .await
    .map_err(Into::into)
}

pub async fn create_embed_response(
    http: &Arc<Http>,
    interaction: &ApplicationCommandInteraction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    interaction
        .create_interaction_response(&http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.add_embed(embed.clone()))
        })
        .await
        .map_err(Into::into)
}

pub async fn edit_embed_response(
    http: &Arc<Http>,
    interaction: &ApplicationCommandInteraction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    interaction
        .edit_original_interaction_response(&http, |message| message.content(" ").add_embed(embed))
        .await
        .map_err(Into::into)
}

pub enum ApplicationCommandOrMessageInteraction {
    ApplicationCommand(ApplicationCommandInteraction),
    Message(MessageInteraction),
}

impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: MessageInteraction) -> Self {
        Self::Message(message)
    }
}

impl From<ApplicationCommandInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: ApplicationCommandInteraction) -> Self {
        Self::ApplicationCommand(message)
    }
}

pub async fn edit_embed_response_poise(ctx: Context<'_>, embed: CreateEmbed) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_original_interaction_response(&ctx.serenity_context().http, |message| {
                message.content(" ").add_embed(embed)
            })
            .await
            .map(|_| Ok(()))?,
        None => create_embed_response_poise(ctx, embed).await, //Err(Box::new(SerenityError::Other("No interaction found"))),
    }
}

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let metadata = track.metadata().clone();

    tracing::warn!("metadata: {:?}", metadata);

    embed.author(|author| author.name(CrackedMessage::NowPlaying));
    metadata
        .title
        .as_ref()
        .map(|title| embed.title(title.clone()));

    metadata
        .source_url
        .as_ref()
        .map(|source_url| embed.url(source_url.clone()));

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    embed.field("Progress", format!(">>> {} / {}", position, duration), true);

    match metadata.channel {
        Some(channel) => embed.field("Channel", format!(">>> {}", channel), true),
        None => embed.field("Channel", ">>> N/A", true),
    };

    metadata
        .thumbnail
        .as_ref()
        .map(|thumbnail| embed.thumbnail(thumbnail));

    let source_url = metadata.source_url.unwrap_or_else(|| {
        tracing::warn!("No source url found for track: {:?}", track);
        "".to_string()
    });

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);
    embed.footer(|f| f.text(footer_text).icon_url(footer_icon_url));

    embed
}

pub async fn create_lyrics_embed_old(track: String, artists: String, lyric: String) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    // let metadata = track_handle.metadata().clone();

    tracing::trace!("lyric: {}", lyric);
    tracing::trace!("track: {}", track);
    tracing::trace!("artists: {}", artists);

    embed.author(|author| author.name(artists));
    embed.title(track);
    embed.description(lyric);

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
            .send(|m| {
                m.embed(|e| {
                    e.title(title.clone());
                    e.author(|a| a.name(author.clone()));
                    e.description(page_getter(0));
                    e.footer(|f| f.text(format!("Page {}/{}", 1, num_pages)))
                });
                m.components(|components| build_nav_btns(components, 0, num_pages))
            })
            .await?;
        reply.into_message().await?
    };

    let mut cib = message
        .await_component_interactions(ctx)
        .timeout(Duration::from_secs(60 * 10))
        .build();

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

        mci.create_interaction_response(&ctx, |r| {
            r.kind(InteractionResponseType::UpdateMessage);
            r.interaction_response_data(|d| {
                d.embed(|e| {
                    e.title(title.clone());
                    e.author(|a| a.name(author.clone()));
                    e.description(page_getter(*page_wlock));
                    e.footer(|f| f.text(format!("Page {}/{}", *page_wlock + 1, num_pages)))
                });
                d.components(|components| build_nav_btns(components, *page_wlock, num_pages))
            })
        })
        .await?;
    }

    message
        .edit(&ctx.serenity_context().http, |edit| {
            let mut embed = CreateEmbed::default();
            embed.description("Lryics timed out, run the command again to see them.");
            edit.set_embed(embed);
            edit.components(|f| f)
        })
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

pub fn get_interaction(ctx: Context<'_>) -> Option<ApplicationCommandInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => Some(x.clone()),
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
    }
}

pub fn get_interaction_new(ctx: Context<'_>) -> Option<ApplicationCommandOrMessageInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => {
                Some(x.clone().into())
            }
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(ctx) => ctx.msg.interaction.clone().map(|x| x.into()),
    }
}

pub fn get_user_id(ctx: &Context) -> serenity::UserId {
    match ctx {
        Context::Application(ctx) => match ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => x.user.id,
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.user.id,
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
    interaction: &ApplicationCommandInteraction,
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
