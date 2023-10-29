use self::serenity::{builder::CreateEmbed, http::Http, model::channel::Message};
use crate::{
    //commands::{build_nav_btns, summon},
    guild::settings::DEFAULT_LYRICS_PAGE_SIZE,
    messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS,
    Context,
    CrackedError,
    Data,
    Error,
};
use ::serenity::{
    all::{ButtonStyle, GuildId, Interaction, InteractionResponseFlags},
    builder::{
        CreateActionRow, CreateButton, CreateEmbedAuthor, CreateEmbedFooter,
        CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
        EditMessage,
    },
    futures::StreamExt,
};
use poise::{
    serenity_prelude::{
        self as serenity, CommandInteraction, Context as SerenityContext, CreateMessage,
        MessageInteraction,
    },
    CommandOrAutocompleteInteraction, CreateReply, FrameworkError, ReplyHandle,
};
use songbird::{input::AuxMetadata, tracks::TrackHandle};
use std::{
    cmp::{max, min},
    ops::Add,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::RwLock;
use url::Url;
const EMBED_PAGE_SIZE: usize = 6;

pub async fn create_log_embed(
    channel: &serenity::ChannelId,
    http: &Arc<Http>,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, Error> {
    let embed = CreateEmbed::default()
        .title(title)
        .description(description)
        .thumbnail(avatar_url);
    // tracing::debug!("sending log embed: {:?}", embed);
    tracing::debug!("thumbnail url: {:?}", avatar_url);

    channel
        .send_message(http, CreateMessage::new().embed(embed))
        .await
        .map_err(Into::into)
}

pub async fn create_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));

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
    interaction: &CommandOrMessageInteraction,
    message: CrackedMessage,
) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    create_embed_response(http, interaction, embed).await
}

pub async fn create_response_text(
    http: &Arc<Http>,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(content);
    create_embed_response(http, interaction, embed).await
}

pub async fn edit_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let embed = CreateEmbed::default().description(format!("{message}"));

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

pub async fn create_embed_response_str(
    ctx: &Context<'_>,
    message_str: String,
) -> Result<Message, Error> {
    ctx.send(
        CreateReply::new()
            .embed(CreateEmbed::new().description(message_str))
            .reply(true),
    )
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
    ctx.send(CreateReply::new().embed(embed))
        // embeds.append(&mut vec![embed]);
        //builder
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
}

pub async fn create_embed_response(
    http: &Arc<Http>,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match interaction {
        CommandOrMessageInteraction::Command(int) => {
            create_reponse_interaction(http, int, embed).await
        }
        _ => Ok(()),
        // Interaction::Message(interaction) => interaction
        //     .create_interaction_response(&http, |response| {
        //         response.interaction_response_data(|message| message.add_embed(embed.clone()))
        //     })
        //     .await
        //     .map_err(Into::into),
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

pub async fn create_reponse_interaction(
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

pub async fn edit_embed_response_poise(ctx: Context<'_>, embed: CreateEmbed) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(interaction) => match interaction {
            CommandOrMessageInteraction::Command(interaction) => match interaction {
                Interaction::Command(interaction) => interaction
                    .edit_response(
                        &ctx.serenity_context().http,
                        EditInteractionResponse::new().content(" ").embed(embed),
                    )
                    .await
                    .map(|_| ())
                    .map_err(Into::into),
                Interaction::Autocomplete(_) => Ok(()),
                Interaction::Component(_) => Ok(()),
                Interaction::Modal(_) => Ok(()),
                Interaction::Ping(_) => Ok(()),
                _ => Ok(()),
            },
            CommandOrMessageInteraction::Message(_) => {
                //     interaction.
                //         (
                //             &ctx.serenity_context().http,
                //             EditInteractionResponse::new().content(" ").embed(embed),
                //         )
                //         .await
                // }
                Ok(())
            }
        },
        None => create_embed_response_poise(ctx, embed).await, //Err(Box::new(SerenityError::Other("No interaction found"))),
    }
}

pub async fn create_now_playing_embed(
    track: &TrackHandle,
    aux_metadata: &AuxMetadata,
) -> CreateEmbed {
    // TrackHandle::metadata(track);
    let metadata = aux_metadata; // track.metadata().clone();

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
        .thumbnail(url::Url::parse(&thumbnail).unwrap())
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

fn build_queue_page(tracks: &[(TrackHandle, AuxMetadata)], page: usize) -> String {
    let description = "".to_string();
    // let start_idx = EMBED_PAGE_SIZE * page;
    // let queue: Vec<&TrackHandle> = tracks
    //     .iter()
    //     .skip(start_idx + 1)
    //     .take(EMBED_PAGE_SIZE)
    //     .collect();

    // if queue.is_empty() {
    //     return String::from(messaging::messages::QUEUE_NO_SONGS);
    // }

    // let mut description = String::new();

    // for (i, t) in queue.iter().enumerate() {
    //     let title = t.metadata().title.as_ref().unwrap();
    //     let url = t.metadata().source_url.as_ref().unwrap();
    //     let duration = get_human_readable_timestamp(t.metadata().duration);

    //     let _ = writeln!(
    //         description,
    //         "`{}.` [{}]({}) • `{}`",
    //         i + start_idx + 1,
    //         title,
    //         url,
    //         duration
    //     );
    // }

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
) -> Result<(), ()> {
    let mut cache_map = data.guild_cache_map.lock().unwrap().clone();

    let cache = cache_map.get_mut(&guild_id).ok_or(())?;
    cache.queue_messages.retain(|(m, _)| m.id != message.id);

    Ok(())
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
                format!("{}:{:02}:{:02}", hours, minutes, seconds)
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

pub fn get_interaction(ctx: Context<'_>) -> Option<CommandOrMessageInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            CommandOrAutocompleteInteraction::Command(x) => Some(
                CommandOrMessageInteraction::Command(Interaction::Command(x.clone())),
            ),
            CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        // Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
        Context::Prefix(ctx) => Some(CommandOrMessageInteraction::Message(
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

pub fn get_user_id(ctx: &Context) -> serenity::UserId {
    match ctx {
        Context::Application(ctx) => ctx.interaction.user().id,
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
        Context::Application(_ctx) => {
            tracing::warn!("summoning via slash command");
            // summon().slash_action.unwrap()(ctx).await
            // FIXME
            Ok(())
        }
        Context::Prefix(_ctx) => {
            tracing::warn!("summoning via prefix command");
            // summon().prefix_action.unwrap()(ctx).await
            // FIXME
            Ok(())
        }
    }
}

pub async fn handle_error(
    ctx: &SerenityContext,
    interaction: &CommandOrMessageInteraction,
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

use songbird::id::ChannelId;
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
