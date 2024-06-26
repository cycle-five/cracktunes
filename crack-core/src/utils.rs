#[cfg(feature = "crack-metrics")]
use crate::metrics::COMMAND_EXECUTIONS;
use crate::{
    commands::{music::doplay::RequestingUser, play_utils::QueryType, MyAuxMetadata},
    db::Playlist,
    guild::settings::DEFAULT_PREMIUM,
    messaging::{
        interface::{create_nav_btns, create_now_playing_embed, requesting_user_to_string},
        message::CrackedMessage,
        messages::{
            INVITE_LINK_TEXT_SHORT, INVITE_URL, PLAYLIST_EMPTY, PLAYLIST_LIST_EMPTY, QUEUE_PAGE,
            QUEUE_PAGE_OF, VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL,
        },
    },
    Context as CrackContext, CrackedError, Data, Error,
};
use ::serenity::{
    all::{
        CacheHttp, ChannelId, ComponentInteractionDataKind, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, EmbedField, GuildId, Interaction, UserId,
    },
    builder::{
        CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateInteractionResponse,
        CreateInteractionResponseMessage, EditInteractionResponse, EditMessage,
    },
    futures::StreamExt,
    http::Http,
    model::channel::Message,
};
use poise::{
    serenity_prelude::{
        self as serenity, CommandInteraction, Context as SerenityContext, CreateMessage,
        MessageInteraction,
    },
    CreateReply, ReplyHandle,
};
use songbird::{input::AuxMetadata, tracks::TrackHandle};
use std::sync::Arc;
use std::{
    cmp::{max, min},
    collections::HashMap,
    fmt::Write,
    ops::Add,
    time::Duration,
};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use url::Url;

use songbird::Call;

pub const EMBED_PAGE_SIZE: usize = 6;

pub fn interaction_to_guild_id(interaction: &Interaction) -> Option<GuildId> {
    match interaction {
        Interaction::Command(int) => int.guild_id,
        Interaction::Component(int) => int.guild_id,
        Interaction::Modal(int) => int.guild_id,
        Interaction::Autocomplete(int) => int.guild_id,
        Interaction::Ping(_) => None,
        _ => None,
    }
}

/// Convert a duration to a string.
pub fn duration_to_string(duration: Duration) -> String {
    let mut secs = duration.as_secs();
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    secs %= 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, secs)
}

/// Create and sends an log message as an embed.
/// FIXME: The avatar_url won't always be available. How do we best handle this?
pub async fn build_log_embed(
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<CreateEmbed, CrackedError> {
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
    guild_name: &str,
    title: &str,
    id: &str,
    description: &str,
    avatar_url: &str,
) -> Result<CreateEmbed, CrackedError> {
    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let footer_str = format!("{} | {} | {}", guild_name, id, now_time_str);
    let footer = CreateEmbedFooter::new(footer_str);
    let author = CreateEmbedAuthor::new(title).icon_url(avatar_url);
    Ok(CreateEmbed::default()
        .author(author)
        // .title(title)
        .description(description)
        // .thumbnail(avatar_url)
        .footer(footer))
}

/// Send a log message as a embed with a thumbnail.
#[cfg(not(tarpaulin_include))]
pub async fn send_log_embed_thumb(
    guild_name: &str,
    channel: &serenity::ChannelId,
    cache_http: &impl CacheHttp,
    id: &str,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, Error> {
    let embed = build_log_embed_thumb(guild_name, title, id, description, avatar_url).await?;

    channel
        .send_message(cache_http, CreateMessage::new().embed(embed))
        .await
        .map_err(Into::into)
}

/// Create and sends an log message as an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_log_embed(
    channel: &serenity::ChannelId,
    http: &impl CacheHttp,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, CrackedError> {
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
    as_embed: bool,
) -> Result<Message, CrackedError> {
    use ::serenity::all::Colour;

    let color = match message {
        CrackedMessage::CrackedError(_) => Colour::RED,
        _ => Colour::BLUE,
    };
    if as_embed {
        let embed = CreateEmbed::default()
            .color(color)
            .description(format!("{message}"));
        send_embed_response_poise(ctx, embed).await
    } else {
        send_nonembed_response_poise(ctx, format!("{message}")).await
    }
}

#[cfg(not(tarpaulin_include))]
/// Sends a reply response as text
pub async fn send_response_poise_text(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, CrackedError> {
    send_response_poise(ctx, message, false).await
}

/// Create an embed to send as a response.
#[cfg(not(tarpaulin_include))]
pub async fn create_response(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    message: CrackedMessage,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    send_embed_response(ctx, interaction, embed).await
}

/// Create an embed to send as a response.
#[cfg(not(tarpaulin_include))]
pub async fn create_response_text(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(content);
    send_embed_response(ctx, interaction, embed).await
}

pub async fn edit_response_poise(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{message}"));

    match get_interaction_new(ctx) {
        Some(interaction) => edit_embed_response(&ctx, &interaction, embed).await,
        None => send_embed_response_poise(ctx, embed).await,
    }
}

pub async fn edit_response(
    http: &impl CacheHttp,
    interaction: &CommandOrMessageInteraction,
    message: CrackedMessage,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{message}"));
    edit_embed_response(http, interaction, embed).await
}

pub async fn edit_response_text(
    http: &impl CacheHttp,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(content);
    edit_embed_response(http, interaction, embed).await
}

/// Send the current track information as an ebmed to the given channel.
#[cfg(not(tarpaulin_include))]
pub async fn send_now_playing(
    channel: ChannelId,
    http: Arc<Http>,
    call: Arc<Mutex<Call>>,
    cur_position: Option<Duration>,
    metadata: Option<AuxMetadata>,
) -> Result<Message, Error> {
    tracing::warn!("locking mutex");
    let mutex_guard = call.lock().await;
    tracing::warn!("mutex locked");
    let msg: CreateMessage = match mutex_guard.queue().current() {
        Some(track_handle) => {
            tracing::warn!("track handle found, dropping mutex guard");
            drop(mutex_guard);
            let requesting_user = get_requesting_user(&track_handle).await;
            let embed = if let Some(metadata2) = metadata {
                create_now_playing_embed_metadata(
                    requesting_user.ok(),
                    cur_position,
                    crate::commands::MyAuxMetadata::Data(metadata2),
                )
            } else {
                create_now_playing_embed(&track_handle).await
            };
            CreateMessage::new().embed(embed)
        },
        None => {
            tracing::warn!("track handle not found, dropping mutex guard");
            drop(mutex_guard);
            CreateMessage::new().content("Nothing playing")
        },
    };
    tracing::warn!("sending message: {:?}", msg);
    channel
        .send_message(Arc::clone(&http), msg)
        .await
        .map_err(|e| e.into())
}

async fn build_embed_fields(elems: Vec<AuxMetadata>) -> Vec<EmbedField> {
    tracing::warn!("num elems: {:?}", elems.len());
    let mut fields = vec![];
    // let tmp = "".to_string();
    for elem in elems.into_iter() {
        let title = elem.title.unwrap_or_default();
        let link = elem.source_url.unwrap_or_default();
        let duration = elem.duration.unwrap_or_default();
        let elem = format!("({}) - {}", link, duration_to_string(duration));
        fields.push(EmbedField::new(format!("[{}]", title), elem, true));
    }
    fields
}

#[cfg(not(tarpaulin_include))]
/// Interactive youtube search and selection.
pub async fn yt_search_select(
    ctx: SerenityContext,
    channel_id: ChannelId,
    metadata: Vec<AuxMetadata>,
) -> Result<QueryType, Error> {
    let res = metadata.iter().map(|x| {
        let title = x.title.clone().unwrap_or_default();
        let link = x.source_url.clone().unwrap_or_default();
        let duration = x.duration.unwrap_or_default();
        let elem = format!("{}: {}", duration_to_string(duration), title);
        let len = min(elem.len(), 99);
        let elem = elem[..len].to_string();
        tracing::warn!("elem: {}", elem);
        (elem, link)
    });
    let rev_map = res
        .clone()
        .map(|(elem, link)| (link, elem))
        .collect::<HashMap<_, _>>();
    // Ask the user for its favorite animal
    let m = channel_id
        .send_message(
            &ctx,
            CreateMessage::new().content("Search results").select_menu(
                CreateSelectMenu::new(
                    "song_select",
                    CreateSelectMenuKind::String {
                        options: res
                            .map(|(x, y)| CreateSelectMenuOption::new(x, y))
                            .collect(),
                    },
                )
                .custom_id("song_select")
                .placeholder("Select Song to Play"),
            ),
        )
        .await?;

    // Wait for the user to make a selection
    // This uses a collector to wait for an incoming event without needing to listen for it
    // manually in the EventHandler.
    let interaction = match m
        .await_component_interaction(&ctx.shard)
        .timeout(Duration::from_secs(60 * 3))
        .await
    {
        Some(x) => x,
        None => {
            m.reply(&ctx, "Timed out").await.unwrap();
            return Err(CrackedError::Other("Timed out").into());
        },
    };

    // data.values contains the selected value from each select menus. We only have one menu,
    // so we retrieve the first
    let url = match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => &values[0],
        _ => panic!("unexpected interaction data kind"),
    };

    tracing::error!("url: {}", url);

    let qt = QueryType::VideoLink(url.to_string());
    tracing::error!("url: {:?}", qt);

    // Acknowledge the interaction and edit the message
    let res = interaction
        .create_response(
            &ctx,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::default().content(CrackedMessage::SongQueued {
                    title: rev_map.get(url).unwrap().to_string(),
                    url: url.to_owned(),
                }),
            ),
        )
        .await
        .map_err(|e| e.into())
        .map(|_| qt);

    m.delete(&ctx).await.unwrap();
    res
}

/// Send the search results to the user.
pub async fn send_search_response(
    ctx: CrackContext<'_>,
    guild_id: GuildId,
    user_id: UserId,
    query: String,
    res: Vec<AuxMetadata>,
) -> Result<Message, CrackedError> {
    use poise::serenity_prelude::Mentionable;
    let author = ctx.author_member().await.unwrap();
    let name = if DEFAULT_PREMIUM {
        author.mention().to_string()
    } else {
        author.display_name().to_string()
    };

    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let fields = build_embed_fields(res).await;
    let author = CreateEmbedAuthor::new(name);
    let title = format!("Search results for: {}", query);
    let footer = CreateEmbedFooter::new(format!("{} * {} * {}", user_id, guild_id, now_time_str));
    let embed = CreateEmbed::new()
        .author(author)
        .title(title)
        .footer(footer)
        .fields(fields.into_iter().map(|f| (f.name, f.value, f.inline)));

    send_embed_response_poise(ctx, embed).await
}

/// Sends a reply response with an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
) -> Result<Message, CrackedError> {
    let is_prefix = crate::is_prefix(ctx);
    let is_ephemeral = !is_prefix;
    let is_reply = is_prefix;
    ctx.send(
        CreateReply::default()
            .embed(embed)
            .ephemeral(is_ephemeral)
            .reply(is_reply),
    )
    .await?
    .into_message()
    .await
    .map_err(Into::into)
}

/// Sends a regular reply response.
#[cfg(not(tarpaulin_include))]
pub async fn send_nonembed_response_poise(
    ctx: CrackContext<'_>,
    text: String,
) -> Result<Message, CrackedError> {
    ctx.send(
        CreateReply::default()
            .content(text)
            .ephemeral(false)
            .reply(true),
    )
    .await?
    .into_message()
    .await
    .map_err(Into::into)
}

pub async fn send_embed_response_prefix(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
) -> Result<Message, CrackedError> {
    send_embed_response_poise(ctx, embed).await
}

pub async fn send_embed_response(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed,
) -> Result<Message, CrackedError> {
    match interaction {
        CommandOrMessageInteraction::Command(int) => {
            tracing::warn!("CommandOrMessageInteraction::Command");
            create_response_interaction(&ctx, &Interaction::Command(int.clone()), embed, false)
                .await
        },
        // Under what circusmtances does this get called?
        CommandOrMessageInteraction::Message(_interaction) => {
            tracing::warn!("CommandOrMessageInteraction::Message");
            ctx.channel_id()
                .send_message(ctx.http(), CreateMessage::new().embed(embed))
                .await
                .map_err(Into::into)
        },
    }
}

pub async fn edit_reponse_interaction(
    http: &impl CacheHttp,
    interaction: &Interaction,
    embed: CreateEmbed,
) -> Result<Message, CrackedError> {
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

/// Create (and send) a response to an interaction.
#[cfg(not(tarpaulin_include))]
pub async fn create_response_interaction(
    cache_http: &impl CacheHttp,
    interaction: &Interaction,
    embed: CreateEmbed,
    _defer: bool,
) -> Result<Message, CrackedError> {
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
            let message = int.get_response(cache_http.http()).await;
            match message {
                Ok(message) => {
                    message
                        .clone()
                        .edit(cache_http, EditMessage::default().embed(embed.clone()))
                        .await?;
                    Ok(message)
                },
                Err(_) => {
                    int.create_response(cache_http, res).await?;
                    let message = int.get_response(cache_http.http()).await?;
                    Ok(message)
                },
            }
        },
        Interaction::Ping(..)
        | Interaction::Component(..)
        | Interaction::Modal(..)
        | Interaction::Autocomplete(..) => Err(CrackedError::Other("not implemented")),
        _ => unimplemented!(),
    }
}

/// Defers a response to an interaction.
/// TODO: use a macro to reduce code here?
pub async fn defer_response_interaction(
    http: impl CacheHttp,
    interaction: &Interaction,
    embed: CreateEmbed,
) -> Result<(), CrackedError> {
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

/// Edit the embed response of the given message.
#[cfg(not(tarpaulin_include))]
pub async fn edit_embed_response2(
    ctx: CrackContext<'_>,
    embed: CreateEmbed,
    mut msg: Message,
) -> Result<Message, Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_response(ctx, EditInteractionResponse::new().add_embed(embed))
            .await
            .map_err(Into::into),
        None => msg
            .edit(ctx, EditMessage::new().embed(embed))
            .await
            .map(|_| msg)
            .map_err(Into::into),
    }
}

/// WHY ARE THERE TWO OF THESE?
pub async fn edit_embed_response(
    http: &impl CacheHttp,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed,
) -> Result<Message, CrackedError> {
    match interaction {
        CommandOrMessageInteraction::Command(int) => {
            edit_reponse_interaction(http, &Interaction::Command(int.clone()), embed).await
        },
        CommandOrMessageInteraction::Message(msg) => match msg {
            Some(_msg) => {
                // Ok(CreateMessage::new().content("edit_embed_response not implemented").)
                Ok(Message::default())
                //    http.edit_origin, new_attachments)
                //     msg.user.id
            },
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
) -> Result<Message, CrackedError> {
    match get_interaction_new(ctx) {
        Some(interaction1) => match interaction1 {
            CommandOrMessageInteraction::Command(interaction2) => {
                // match interaction2 {
                //     Interaction::Command(interaction3) => {
                //         tracing::warn!("CommandInteraction");
                interaction2
                    .edit_response(
                        &ctx.serenity_context().http,
                        EditInteractionResponse::new().content(" ").embed(embed),
                    )
                    .await
                    .map_err(Into::into)
                //     },
                //     _ => Err(CrackedError::Other("not implemented")),
            },
            CommandOrMessageInteraction::Message(_) => send_embed_response_poise(ctx, embed).await,
        },
        None => send_embed_response_poise(ctx, embed).await,
    }
}

/// Gets the requesting user from the typemap of the track handle.
pub async fn get_requesting_user(track: &TrackHandle) -> Result<serenity::UserId, CrackedError> {
    let user = match track.typemap().read().await.get::<RequestingUser>() {
        Some(RequestingUser::UserId(user)) => *user,
        None => {
            tracing::warn!("No user found for track: {:?}", track);
            return Err(CrackedError::NoUserAutoplay);
        },
    };
    Ok(user)
}

/// Gets the metadata from a track.
pub async fn get_track_metadata(track: &TrackHandle) -> AuxMetadata {
    let metadata = {
        let map = track.typemap().read().await;
        let my_metadata = match map.get::<crate::commands::MyAuxMetadata>() {
            Some(my_metadata) => my_metadata,
            None => {
                tracing::warn!("No metadata found for track: {:?}", track);
                return AuxMetadata::default();
            },
        };

        match my_metadata {
            crate::commands::MyAuxMetadata::Data(metadata) => metadata.clone(),
        }
    };
    metadata
}

/// Creates an embed from a CrackedMessage and sends it as an embed.
pub fn create_now_playing_embed_metadata(
    requesting_user: Option<UserId>,
    cur_position: Option<Duration>,
    metadata: MyAuxMetadata,
) -> CreateEmbed {
    let MyAuxMetadata::Data(metadata) = metadata;
    tracing::warn!("metadata: {:?}", metadata);

    let title = metadata.title.clone().unwrap_or_default();

    let source_url = metadata.source_url.clone().unwrap_or_default();

    let position = get_human_readable_timestamp(cur_position);
    let duration = get_human_readable_timestamp(metadata.duration);

    let progress_field = ("Progress", format!(">>> {} / {}", position, duration), true);

    let channel_field: (&'static str, String, bool) = match requesting_user {
        Some(user_id) => (
            "Requested By",
            format!(">>> {}", requesting_user_to_string(user_id)),
            true,
        ),
        None => {
            tracing::warn!("No user id");
            ("Requested By", ">>> N/A".to_string(), true)
        },
    };
    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();

    let (footer_text, footer_icon_url, vanity) = get_footer_info(&source_url);

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
        .description(vanity)
        .footer(CreateEmbedFooter::new(footer_text).icon_url(footer_icon_url))
}

/// Creates an embed for the first N metadata in the queue.
async fn build_queue_page_metadata(metadata: &[MyAuxMetadata], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    let queue: Vec<&MyAuxMetadata> = metadata
        .iter()
        .skip(start_idx)
        .take(EMBED_PAGE_SIZE)
        .collect();

    if queue.is_empty() {
        return String::from(PLAYLIST_EMPTY);
    }

    let mut description = String::new();

    for (i, &t) in queue.iter().enumerate() {
        let MyAuxMetadata::Data(t) = t;
        let title = t.title.clone().unwrap_or_default();
        let url = t.source_url.clone().unwrap_or_default();
        let duration = get_human_readable_timestamp(t.duration);

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

/// Calculate the number of pages needed to display all the tracks.
pub fn calculate_num_pages<T>(tracks: &[T]) -> usize {
    let num_pages = ((tracks.len() as f64 - 1.0) / EMBED_PAGE_SIZE as f64).ceil() as usize;
    max(1, num_pages)
}

/// Forget the current cache of queue messages we need to update.
pub async fn forget_queue_message(
    data: &Data,
    message: &Message,
    guild_id: GuildId,
) -> Result<(), CrackedError> {
    let mut cache_map = data.guild_cache_map.lock().await;

    let cache = cache_map
        .get_mut(&guild_id)
        .ok_or(CrackedError::NoGuildId)?;
    cache.queue_messages.retain(|(m, _)| m.id != message.id);

    Ok(())
}

pub async fn build_playlist_list_embed(playlists: &[Playlist], page: usize) -> CreateEmbed {
    let content = if !playlists.is_empty() {
        let start_idx = EMBED_PAGE_SIZE * page;
        let playlists: Vec<&Playlist> = playlists.iter().skip(start_idx).take(10).collect();

        let mut description = String::new();

        for (i, &playlist) in playlists.iter().enumerate() {
            let _ = writeln!(
                description,
                // "`{}.` [{}]({})",
                "`{}.` {} ({})",
                i + start_idx + 1,
                playlist.name,
                playlist.id
            );
        }

        description
    } else {
        PLAYLIST_LIST_EMPTY.to_string()
    };

    CreateEmbed::default()
        .title("Playlists")
        .description(content)
    //     .footer(CreateEmbedFooter::new(format!(
    //         "{} {} {} {}",
    //         QUEUE_PAGE,
    //         page + 1,
    //         QUEUE_PAGE_OF,
    //         calculate_num_pages(playlists),
    //     )))
}

pub async fn build_tracks_embed_metadata(
    playlist_name: String,
    metadata_arr: &[MyAuxMetadata],
    page: usize,
) -> CreateEmbed {
    CreateEmbed::default()
        //.field("Playlist:", &playlist_name, true)
        .field(
            playlist_name,
            build_queue_page_metadata(metadata_arr, page).await,
            false,
        )
        .footer(CreateEmbedFooter::new(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(metadata_arr),
        )))
}

/// Creates and sends a paged embed.
pub async fn create_paged_embed(
    ctx: CrackContext<'_>,
    author: String,
    title: String,
    content: String,
    page_size: usize,
) -> Result<(), CrackedError> {
    let page_getter = create_page_getter_newline(&content, page_size);
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
                    .components(create_nav_btns(0, num_pages)),
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
                    .components(create_nav_btns(*page_wlock, num_pages)),
            ),
        )
        .await?;
    }

    message
        .edit(
            &ctx.serenity_context().http,
            EditMessage::default()
                .embed(CreateEmbed::default().description(CrackedMessage::PaginationComplete)),
        )
        .await
        .unwrap();

    Ok(())
}

/// Split a str into chunks
pub fn split_string_into_chunks(string: &str, chunk_size: usize) -> Vec<String> {
    string
        .chars()
        .collect::<Vec<char>>()
        .chunks(chunk_size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// Splits a String chunks of a given size, but tries to split on a newline if possible.
pub fn split_string_into_chunks_newline(string: &str, chunk_size: usize) -> Vec<String> {
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
            },
            None => chunk,
        };
        chunks.push(chunk.to_string());
        cur = next;
    }

    chunks
}

/// Creates a closure that returns a page of a chunked string.
pub fn create_page_getter(string: &str, chunk_size: usize) -> impl Fn(usize) -> String {
    let chunks = split_string_into_chunks(string, chunk_size);
    move |page| {
        let page = page % chunks.len();
        chunks[page].clone()
    }
}

/// Creates a closure that returns a page of a chunked string, but tries to split on a newline if possible.
pub fn create_page_getter_newline(
    string: &str,
    chunk_size: usize,
) -> impl Fn(usize) -> String + '_ {
    let chunks = split_string_into_chunks_newline(string, chunk_size);
    move |page| {
        let page = page % chunks.len();
        chunks[page].clone()
    }
}

pub fn get_footer_info(url: &str) -> (String, String, String) {
    let vanity = format!(
        "[{}]({}) • [{}]({})",
        VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL, INVITE_LINK_TEXT_SHORT, INVITE_URL,
    );
    let url_data = match Url::parse(url) {
        Ok(url_data) => url_data,
        Err(_) => {
            return (
                "Streaming via unknown".to_string(),
                "https://www.google.com/s2/favicons?domain=unknown".to_string(),
                vanity,
            )
        },
    };
    let domain = url_data.host_str().unwrap();

    // remove www prefix because it looks ugly
    let domain = domain.replace("www.", "");

    (
        format!("Streaming via {}", domain),
        format!("https://www.google.com/s2/favicons?domain={}", domain),
        vanity,
    )
}

/// Converts a duration into a human readable timestamp
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
        },
        None => "∞".to_string(),
    }
}

use serenity::prelude::SerenityError;

/// Check if a subdomian is from the same domain.
pub fn compare_domains(domain: &str, subdomain: &str) -> bool {
    subdomain == domain || subdomain.ends_with(domain)
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
pub fn check_msg(result: Result<Message, Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

#[cfg(not(tarpaulin_include))]
/// Takes a Result ReplyHandle and logs the error if it's an Err.
pub fn check_reply(result: Result<ReplyHandle, SerenityError>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

/// Checks a Result and logs the error if it's an Err.
pub fn check_interaction(result: Result<(), Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub enum CommandOrMessageInteraction {
    Command(CommandInteraction),
    Message(Option<Box<MessageInteraction>>),
}

pub fn get_interaction(ctx: CrackContext<'_>) -> Option<CommandInteraction> {
    match ctx {
        CrackContext::Application(app_ctx) => app_ctx.interaction.clone().into(),
        // match app_ctx.interaction {
        //     CommandOrAutocompleteInteraction::Command(x) => Some(x.clone()),
        //     CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        // },
        // CrackContext::Prefix(prefix_ctx) => Some(prefix_ctx.msg.interaction.into()),
        CrackContext::Prefix(_ctx) => None,
    }
}

pub fn get_interaction_new(ctx: CrackContext<'_>) -> Option<CommandOrMessageInteraction> {
    match ctx {
        CrackContext::Application(app_ctx) => {
            Some(CommandOrMessageInteraction::Command(
                app_ctx.interaction.clone(),
            ))
            // match app_ctx.interaction {
            // CommandOrAutocompleteInteraction::Command(x) => Some(
            //     CommandOrMessageInteraction::Command(Interaction::Command(x.clone())),
            // ),
            // CommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        // Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
        CrackContext::Prefix(ctx) => Some(CommandOrMessageInteraction::Message(
            ctx.msg.interaction.clone(),
        )),
    }
}

/// Get the user id from a context.
pub fn get_user_id(ctx: &CrackContext) -> serenity::UserId {
    match ctx {
        CrackContext::Application(ctx) => ctx.interaction.user.id,
        CrackContext::Prefix(ctx) => ctx.msg.author.id,
    }
}

pub async fn handle_error(
    ctx: CrackContext<'_>,
    interaction: &CommandOrMessageInteraction,
    err: CrackedError,
) {
    create_response_text(ctx, interaction, &format!("{err}"))
        .await
        .expect("failed to create response");
}

#[cfg(feature = "crack-metrics")]
pub fn count_command(command: &str, is_prefix: bool) {
    tracing::warn!("counting command: {}, {}", command, is_prefix);
    match COMMAND_EXECUTIONS
        .get_metric_with_label_values(&[command, if is_prefix { "prefix" } else { "slash" }])
    {
        Ok(metric) => {
            metric.inc();
        },
        Err(e) => {
            tracing::error!("Failed to get metric: {}", e);
        },
    };
}
#[cfg(not(feature = "crack-metrics"))]
pub fn count_command(command: &str, is_prefix: bool) {
    tracing::warn!(
        "crack-metrics feature not enabled!\ncommand: {}, {}",
        command,
        is_prefix
    );
}

pub fn get_guild_name(ctx: &SerenityContext, guild_id: serenity::GuildId) -> Option<String> {
    let guild = ctx.cache.guild(guild_id)?;
    Some(guild.name.clone())
}

#[cfg(test)]
mod test {

    use ::serenity::{all::Button, builder::CreateActionRow};

    use crate::messaging::interface::create_single_nav_btn;

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
        let (text, icon_url, vanity) = get_footer_info("https://www.rust-lang.org/");
        assert_eq!(text, "Streaming via rust-lang.org");
        assert!(icon_url.contains("rust-lang.org"));
        assert!(vanity.contains("vote"));
    }

    #[test]
    fn test_build_single_nav_btn() {
        let creat_btn = create_single_nav_btn("<<", true);
        let s = serde_json::to_string_pretty(&creat_btn).unwrap();
        println!("s: {}", s);
        let btn = serde_json::from_str::<Button>(&s).unwrap();

        assert_eq!(btn.label, Some("<<".to_string()));
        // assert_eq!(btn.style, ButtonStyle::Primary);
        assert_eq!(btn.disabled, true);
    }

    #[test]
    fn test_build_nav_btns() {
        let nav_btns_vev = create_nav_btns(0, 1);
        if let CreateActionRow::Buttons(nav_btns) = &nav_btns_vev[0] {
            let mut btns = Vec::new();
            for btn in nav_btns {
                let s = serde_json::to_string_pretty(&btn).unwrap();
                println!("s: {}", s);
                let btn = serde_json::from_str::<Button>(&s).unwrap();
                btns.push(btn);
            }
            let s = serde_json::to_string_pretty(&nav_btns).unwrap();
            println!("s: {}", s);
            let btns = serde_json::from_str::<Vec<Button>>(&s).unwrap();

            assert_eq!(btns.len(), 4);
            let btn = &btns[0];
            assert_eq!(btns[0], btn.clone());
        } else {
            assert!(false);
        }
    }
}
