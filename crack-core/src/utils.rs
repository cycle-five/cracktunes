use crate::http_utils::CacheHttpExt;
use crate::http_utils::SendMessageParams;
use crate::messaging::courier::{self, Destination};
use crate::messaging::format::{duration_text, TrackLabel, INLINE_TITLE_MAX};
use crate::messaging::messages::{
    SEARCH_MENU_NOT_REMOVED, SEARCH_RESULTS_NOT_POSTED, TRACK_UNTITLED,
};
use crate::messaging::render::{RenderCx, Rendered};
use crate::messaging::transport::{DiscordTransport, Transport};
#[cfg(feature = "crack-metrics")]
use crate::metrics::COMMAND_EXECUTIONS;
use crate::poise_ext::PoiseContextExt;
use crate::{
    db::Playlist,
    messaging::{
        interface::create_nav_btns,
        message::CrackedMessage,
        messages::{
            INVITE_LINK_TEXT_SHORT, INVITE_URL, PLAYLISTS, PLAYLIST_EMPTY, PLAYLIST_LIST_EMPTY,
            QUEUE_PAGE, QUEUE_PAGE_OF, VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL,
        },
    },
    Context as CrackContext, CrackedError, CrackedResult, Data, Error,
};
use ::serenity::all::MessageInteractionMetadata;
use ::serenity::small_fixed_array::FixedString;
use ::serenity::{
    all::{
        CacheHttp, Colour, ComponentInteractionDataKind, CreateActionRow, CreateComponent,
        CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, GenericChannelId, GuildId,
        Interaction,
    },
    builder::{
        CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateInteractionResponse,
        CreateInteractionResponseMessage, EditInteractionResponse,
    },
    futures::StreamExt,
    model::channel::Message,
};
use anyhow::Result;
use crack_types::NewAuxMetadata;
use crack_types::QueryType;
use poise::{
    serenity_prelude::{self as serenity, CommandInteraction, Context as SerenityContext},
    ReplyHandle,
};
use serenity::all::UserId;
#[allow(deprecated)]
use songbird::{input::AuxMetadata, tracks::TrackHandle};
use std::sync::Arc;
use std::{
    cmp::{max, min},
    collections::HashMap,
    fmt::Write,
    ops::Add,
    time::Duration,
};
use tokio::sync::RwLock;
use url::Url;

pub const EMBED_PAGE_SIZE: usize = 6;
// This term gets appended to search queries in the default mode to try to find the album version of a song.
// pub const MUSIC_SEARCH_SUFFIX: &str = "album version";
// FIXME: Whether we use this or not it doesnt' go here.
pub const MUSIC_SEARCH_SUFFIX: &str = r#"\"topic\""#;

/// FIXME: What is "cold"? And mustrify it.
#[cold]
fn create_err(line: u32, file: &str) -> anyhow::Error {
    anyhow::anyhow!("Unexpected None value on line {line} in {file}",)
}

pub trait OptionTryUnwrap<T> {
    fn try_unwrap(self) -> CrackedResult<T>;
}

impl<T> OptionTryUnwrap<T> for Option<T> {
    #[track_caller]
    fn try_unwrap(self) -> CrackedResult<T> {
        match self {
            Some(v) => Ok(v),
            None => Err({
                let location = std::panic::Location::caller();
                create_err(location.line(), location.file()).into()
            }),
        }
    }
}

/// FIXME: This really should just be used as the method on the struct.
/// Leaving this out of convenience, eventually it should be removed.
pub async fn get_guild_name(cache_http: impl CacheHttp, guild_id: GuildId) -> Option<FixedString> {
    cache_http.guild_name_from_guild_id(guild_id).await.ok()
}

/// Sends a reply response, possibly as an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_reply<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    message: CrackedMessage,
    as_embed: bool,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    ctx.send_reply(message, as_embed).await
}

/// Sends a reply response, possibly as an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_reply_owned(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
    as_embed: bool,
) -> Result<ReplyHandle<'_>, CrackedError> {
    ctx.send_reply_owned(message, as_embed).await
}

/// Sends a regular reply response.
#[cfg(not(tarpaulin_include))]
pub async fn send_nonembed_reply(
    ctx: &CrackContext<'_>,
    msg: CrackedMessage,
) -> Result<Message, CrackedError> {
    let color = Colour::from(&msg);

    let params = SendMessageParams::default()
        .with_color(color)
        .with_msg(msg)
        .with_as_embed(false);

    let handle = ctx.send_message(params).await?;
    Ok(handle.into_message().await?)
}

#[cfg(not(tarpaulin_include))]
/// Edit an embed response with a CrackedMessage.
pub async fn edit_response_poise(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{message}"));

    match get_interaction_new(&ctx) {
        Some(interaction) => edit_embed_response(&ctx, &interaction, embed).await,
        None => match send_embed_response_poise(ctx, embed).await {
            Ok(msg) => msg.into_message().await.map_err(Into::into),
            Err(e) => Err(e),
        },
    }
}

#[cfg(not(tarpaulin_include))]
/// Edit an embed response from a CommandOrMessageInteraction with a str.
pub async fn edit_response_text(
    http: &impl CacheHttp,
    interaction: &CommandOrMessageInteraction,
    content: &str,
) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(content);
    edit_embed_response(http, interaction, embed).await
}

use poise::serenity_prelude::CollectComponentInteractions;

/// A search hit as a select-menu option, `(label, value)`. The label is plain
/// text (a select menu renders no markdown), `length: title` or just the title
/// when the length is unknown, cut to 99 characters -- characters, not bytes:
/// a byte cut panicked mid-character on a title in Japanese.
fn search_option(hit: &AuxMetadata) -> (String, String) {
    // A select option needs a non-empty label; Discord rejects the whole menu
    // otherwise. Plain text, not escaped: a select menu renders no markdown.
    let title = match hit.title.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_owned(),
        _ => TRACK_UNTITLED.to_owned(),
    };
    let link = hit.source_url.clone().unwrap_or_default();
    let label = match duration_text(hit.duration) {
        Some(length) => format!("{length}: {title}"),
        None => title,
    };
    (label.chars().take(99).collect(), link)
}

#[cfg(not(tarpaulin_include))]
/// Interactive youtube search and selection.
pub async fn yt_search_select(
    ctx: SerenityContext,
    channel_id: GenericChannelId,
    metadata: Vec<AuxMetadata>,
) -> Result<QueryType, Error> {
    let res = metadata.iter().map(|x| {
        let (elem, link) = search_option(x);
        tracing::warn!("elem: {}", elem);
        (elem, link)
    });
    let rev_map = res
        .clone()
        .map(|(elem, link)| (link, elem))
        .collect::<HashMap<_, _>>();
    let menu = CreateSelectMenu::new(
        "song_select",
        CreateSelectMenuKind::String {
            options: res
                .map(|(x, y)| CreateSelectMenuOption::new(x, y))
                .collect(),
        },
    )
    .custom_id("song_select")
    .placeholder("Select Song to Play");
    let out = Rendered::text("Search results").with_components(vec![CreateComponent::ActionRow(
        CreateActionRow::SelectMenu(menu),
    )]);
    let transport = DiscordTransport::of(&ctx);
    // The collector needs the menu's id, so this send is the fallible one.
    let menu_id = courier::post_message(&transport, channel_id, &out)
        .await
        .map_err(|err| {
            tracing::warn!("search: the results menu was not posted: {err:?}");
            CrackedError::Other(SEARCH_RESULTS_NOT_POSTED)
        })?;

    // Wait for the user to make a selection
    // This uses a collector to wait for an incoming event without needing to listen for it
    // manually in the EventHandler.
    let interaction = match menu_id
        .collect_component_interactions(&ctx)
        .timeout(Duration::from_secs(60 * 3))
        .await
    {
        Some(x) => x,
        None => {
            courier::post(
                &ctx.data::<Data>(),
                &transport,
                Destination::Channel(channel_id),
                &CrackedMessage::Other("Timed out".to_owned()),
                &RenderCx::now(),
            )
            .await;
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
            ctx.http(),
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::default().content(CrackedMessage::SongQueued(
                    TrackLabel {
                        title: Some(rev_map.get(url).unwrap().to_string()),
                        url: Some(url.to_owned()),
                        duration: None,
                    },
                )),
            ),
        )
        .await
        .map_err(|e| e.into())
        .map(|_| qt);

    transport.delete(channel_id, menu_id).await.map_err(|err| {
        tracing::warn!("search: the results menu was not removed: {err:?}");
        CrackedError::Other(SEARCH_MENU_NOT_REMOVED)
    })?;
    res
}

/// Sends a reply response with an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise<'ctx>(
    ctx: CrackContext<'ctx>,
    embed: CreateEmbed<'ctx>,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    send_embed_response_poise_as(ctx, embed, false).await
}

/// [`send_embed_response_poise`], ephemeral when `ephemeral` is set.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise_as<'ctx>(
    ctx: CrackContext<'ctx>,
    embed: CreateEmbed<'ctx>,
    ephemeral: bool,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    courier::reply_rendered(ctx, Rendered::embed(embed.into_owned()), ephemeral).await
}

pub async fn edit_reponse_interaction(
    http: &impl CacheHttp,
    interaction: &Interaction,
    embed: CreateEmbed<'_>,
) -> Result<Message, CrackedError> {
    match interaction {
        Interaction::Command(int) => int
            .edit_response(
                http.http(),
                EditInteractionResponse::new().embed(embed.clone()),
            )
            .await
            .map_err(Into::into),
        Interaction::Component(int) => int
            .edit_response(
                http.http(),
                EditInteractionResponse::new().embed(embed.clone()),
            )
            .await
            .map_err(Into::into),
        Interaction::Modal(int) => int
            .edit_response(
                http.http(),
                EditInteractionResponse::new().embed(embed.clone()),
            )
            .await
            .map_err(Into::into),
        Interaction::Autocomplete(int) => int
            .edit_response(
                http.http(),
                EditInteractionResponse::new().embed(embed.clone()),
            )
            .await
            //.map(|_| Message::default())
            .map_err(Into::into),
        Interaction::Ping(_int) => Ok(Message::default()),
        _ => todo!(),
    }
}

/// Edit the message `msg` points to.
///
/// `content` is for text that must survive when the embed does not. 🪤 A
/// channel without `EMBED_LINKS` has its embeds stripped by Discord, so a
/// notice about that very permission is invisible if it rides inside one.
///
/// 🪤 This edits **the handle it is given**, for slash and prefix alike. The
/// slash branch used to ignore `msg` and PATCH `@original` instead, which is
/// the right message only when `msg` IS the initial response. A `/play` that
/// has to join answers first ("Summoned", or a defer), so its `🔎 Searching…`
/// placeholder is a followup: the edit overwrote the join announcement with
/// the now-playing embed and left the placeholder stuck forever (#494) -- on
/// the success path, and only when joining, so it looked intermittent.
/// poise's `ReplyHandle::edit` already routes both cases (`followup: None` →
/// edit the response, `Some` → edit that followup).
///
/// Returns nothing: both callers discarded the `Message`, and producing one
/// for a slash command costs an extra GET after the edit.
#[cfg(not(tarpaulin_include))]
pub async fn edit_embed_response2(
    ctx: CrackContext<'_>,
    embed: CreateEmbed<'_>,
    msg: ReplyHandle<'_>,
    content: Option<String>,
) -> Result<(), Error> {
    let mut out = Rendered::embed(embed.into_owned());
    if let Some(content) = content {
        out = out.with_content(content);
    }
    courier::edit_rendered(ctx, &msg, out).await?;
    Ok(())
}

/// WHY ARE THERE TWO OF THESE?
pub async fn edit_embed_response(
    http: &impl CacheHttp,
    interaction: &CommandOrMessageInteraction,
    embed: CreateEmbed<'_>,
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

// #[allow(deprecated)]
// pub enum ApplicationCommandOrMessageInteraction {
//     Command(CommandInteraction),
//     Message(MessageReaction),
// }

// #[allow(deprecated)]
// impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
//     fn from(message: MessageReaction) -> Self {
//         Self::Message(message)
//     }
// }

// impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
//     fn from(message: MessageInteraction) -> Self {
//         Self::ApplicationCommand(message)
//     }
// }

pub async fn edit_embed_response_poise(
    ctx: CrackContext<'_>,
    embed: CreateEmbed<'_>,
) -> Result<Message, CrackedError> {
    let reply_handle = match get_interaction_new(&ctx) {
        Some(interaction1) => match interaction1 {
            CommandOrMessageInteraction::Command(interaction2) => {
                return interaction2
                    .edit_response(
                        &ctx.serenity_context().http,
                        EditInteractionResponse::new().content(" ").embed(embed),
                    )
                    .await
                    .map_err(Into::into);
                //     },
                //     _ => Err(CrackedError::Other("not implemented")),
            },
            CommandOrMessageInteraction::Message(_) => send_embed_response_poise(ctx, embed).await,
        },
        None => send_embed_response_poise(ctx, embed).await,
    };
    reply_handle?.into_message().await.map_err(Into::into)
}

//use tokio::sync::RwLock;
/// Modifiable data struct for the track information.
#[derive(Clone, Debug, Default)]
pub struct TrackData {
    pub user_id: Arc<RwLock<Option<UserId>>>,
    pub aux_metadata: Arc<RwLock<Option<AuxMetadata>>>,
}

unsafe impl Send for TrackData {}
unsafe impl Sync for TrackData {}

impl TrackData {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::new(RwLock::new(Some(UserId::new(1)))),
            aux_metadata: Arc::new(RwLock::new(None)),
        })
    }

    pub fn with_user_id(self: Arc<Self>, user_id: UserId) -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::new(RwLock::new(Some(user_id))),
            aux_metadata: Arc::clone(&self.aux_metadata),
        })
    }

    pub fn with_metadata(self: Arc<Self>, md: AuxMetadata) -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::clone(&self.user_id),
            aux_metadata: Arc::new(RwLock::new(Some(md))),
        })
    }
}

// impl Default for TrackData {
//     fn default() -> Self {
//         *Self::new().clone()
//     }
// }
// pub type ArcTrackData = Arc<TrackDataInner>;

// impl std::ops::DerefMut for TrackData {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         Arc::get_mut(self).unwrap()
//     }
// }

/// The [`TrackData`] on a queued track.
///
/// The crate's only read of `TrackHandle::data`, which `clippy.toml` bans
/// everywhere else: it panics on a track built without `TrackData`, and only
/// `queue::new_track` guarantees there is some.
#[allow(clippy::disallowed_methods)]
pub(crate) fn track_data(track: &TrackHandle) -> Arc<TrackData> {
    track.data::<TrackData>()
}

/// Gets the requesting user from the typemap of the track handle.
pub async fn get_requesting_user(track: &TrackHandle) -> Result<serenity::UserId, CrackedError> {
    let data = track_data(track);
    let lock = data.user_id.read().await;
    lock.ok_or(CrackedError::NoUserAutoplay)
}

/// Gets the metadata from a track.
pub async fn get_track_handle_metadata(track: &TrackHandle) -> Result<AuxMetadata, CrackedError> {
    let data = track_data(track);
    let lock = data.aux_metadata.read().await;
    lock.clone().ok_or(CrackedError::NoMetadata)
}

/// Sets the metadata for a track.
pub async fn set_track_handle_metadata(
    track: &mut TrackHandle,
    metadata: AuxMetadata,
) -> Result<(), CrackedError> {
    let data = track_data(track);
    let mut lock = data.aux_metadata.write().await;
    *lock = Some(metadata);
    Ok(())
}

/// Sets the requesting user for a track.
pub async fn set_track_handle_requesting_user(
    track: &mut TrackHandle,
    user_id: serenity::UserId,
) -> Result<(), CrackedError> {
    let data = track_data(track);
    let mut lock = data.user_id.write().await;
    *lock = Some(user_id);
    Ok(())
}

/// Creates an embed for the first N metadata in the queue.
async fn build_queue_page_metadata(metadata: &[NewAuxMetadata], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    let queue: Vec<&NewAuxMetadata> = metadata
        .iter()
        .skip(start_idx)
        .take(EMBED_PAGE_SIZE)
        .collect();

    if queue.is_empty() {
        return String::from(PLAYLIST_EMPTY);
    }

    let mut description = String::new();

    for (i, &t) in queue.iter().enumerate() {
        let NewAuxMetadata(t) = t;
        let label = TrackLabel::from_metadata(t);

        // An unknown duration is left out, not shown as 00:00.
        let _ = writeln!(
            description,
            "`{}.` {}{}",
            i + start_idx + 1,
            label.linked(INLINE_TITLE_MAX),
            duration_text(label.duration)
                .map(|d| format!(" • `{d}`"))
                .unwrap_or_default(),
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
    data: Arc<Data>,
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

pub async fn build_playlist_list_embed(playlists: &[Playlist], page: usize) -> CreateEmbed<'_> {
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

    CreateEmbed::default().title(PLAYLISTS).description(content)
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
    metadata_arr: &[NewAuxMetadata],
    page: usize,
) -> CreateEmbed<'_> {
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
    author: FixedString<u8>,
    title: String,
    content: String,
    page_size: usize,
    style: PagedStyle,
) -> CrackedResult<()> {
    let pages = build_pages(&content, page_size, style.fenced);
    let num_pages = pages.len();
    let page_getter = |i: usize| pages[i % num_pages].clone();
    let page: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));

    let _x: Result<(), CrackedError> = {
        let reply_handle = {
            courier::reply_rendered(
                ctx,
                Rendered::embed(
                    CreateEmbed::new()
                        .title(title.clone())
                        .author(CreateEmbedAuthor::new(author.clone()))
                        .description(page_getter(0))
                        .footer(CreateEmbedFooter::new(format!("Page {}/{}", 1, num_pages))),
                )
                .with_components(create_nav_btns(0, num_pages)),
                style.ephemeral,
            )
            .await?
        };
        // let mut message = {
        //     let reply = ctx.clone().send(create_reply).await?;
        //     reply.into_message().await?
        // };
        // let reply_handle = ctx.clone().send(create_reply).await?;
        // drop(create_reply);
        let mut cib = reply_handle
            .clone()
            .into_message()
            .await?
            .id
            .collect_component_interactions(ctx.serenity_context())
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

            let flipped = Rendered::embed(
                CreateEmbed::new()
                    .title(title.clone())
                    .author(CreateEmbedAuthor::new(author.clone()))
                    .description(page_getter(*page_wlock))
                    .footer(CreateEmbedFooter::new(format!(
                        "Page {}/{}",
                        *page_wlock + 1,
                        num_pages
                    ))),
            )
            .with_components(create_nav_btns(*page_wlock, num_pages));
            mci.create_response(
                ctx.http(),
                CreateInteractionResponse::UpdateMessage(flipped.to_interaction_message()),
            )
            .await?;
        }

        if let Err(e) =
            courier::edit_reply(ctx, &reply_handle, CrackedMessage::PaginationComplete).await
        {
            tracing::warn!("could not mark pagination complete (message dismissed?): {e}");
        }
        Ok(())
    };

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
        // Never slice inside a multi-byte char: snap down, or up if that stalls.
        while !string.is_char_boundary(next) {
            next -= 1;
        }
        if next <= cur {
            next = cur + 1;
            while !string.is_char_boundary(next) {
                next += 1;
            }
        }
        let chunk = &string[cur..next];
        // The final window is taken whole; only a window cut short looks for a line end.
        let newline_index = if next < end { chunk.rfind('\n') } else { None };
        let chunk = match newline_index {
            Some(index) => {
                next = index + cur + 1;
                &chunk[..index]
            },
            None => chunk,
        };
        chunks.push(chunk.to_string());
        //chunks.push(format!("```\n{}\n```", chunk));
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

/// How a paged embed is sent and rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagedStyle {
    /// Send the reply so only the caller sees it.
    pub ephemeral: bool,
    /// Wrap each page in a ```md code fence (turns off mentions/timestamps rendering).
    pub fenced: bool,
}

impl Default for PagedStyle {
    fn default() -> Self {
        Self {
            ephemeral: false,
            fenced: true,
        }
    }
}

/// Splits `string` into pages on newlines where possible, optionally fenced.
/// Always returns at least one page; the page count is `pages.len()`.
pub fn build_pages(string: &str, chunk_size: usize, fenced: bool) -> Vec<String> {
    let mut chunks = split_string_into_chunks_newline(string, chunk_size);
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    if fenced {
        chunks
            .into_iter()
            .map(|c| format!("```md\n{}\n```", c))
            .collect()
    } else {
        chunks
    }
}

/// Build the strings used for the footer of an embed from a given url.
pub fn build_footer_info(url: &str) -> (String, String, String) {
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

// `Command(CommandInteraction)` is ~776 bytes against a boxed `Message` variant.
// Deferred with the other large-variant cleanups.
#[allow(deprecated, clippy::large_enum_variant)]
pub enum CommandOrMessageInteraction {
    Command(CommandInteraction),
    Message(Option<Box<MessageInteractionMetadata>>),
    //Message(Option<Box<MessageInteraction>>),
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

#[allow(deprecated)]
pub fn get_interaction_new(ctx: &CrackContext<'_>) -> Option<CommandOrMessageInteraction> {
    match ctx {
        CrackContext::Application(app_ctx) => Some(CommandOrMessageInteraction::Command(
            app_ctx.interaction.clone(),
        )),
        CrackContext::Prefix(ctx) => Some(CommandOrMessageInteraction::Message(
            ctx.msg.interaction_metadata.clone(),
        )),
    }
}

// pub async fn handle_error(
//     ctx: CrackContext<'_>,
//     interaction: &CommandOrMessageInteraction,
//     err: CrackedError,
// ) {
//     create_response_text(&ctx, interaction, &format!("{err}"))
//         .await
//         .expect("failed to create response");
// }

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

/// Get the guild id from an interaction.
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

#[cfg(test)]
mod search_option_tests {
    use super::search_option;
    use songbird::input::AuxMetadata;
    use std::time::Duration;

    /// 🪤 The label was cut at 99 *bytes*: a title in Japanese (or with an
    /// emoji) split a character and panicked the search.
    #[test]
    fn a_search_option_is_cut_by_characters() {
        let hit = AuxMetadata {
            title: Some("あ".repeat(120)),
            source_url: Some("https://youtu.be/x".into()),
            duration: Some(Duration::from_secs(273)),
            ..Default::default()
        };
        let (label, value) = search_option(&hit);
        assert_eq!(label.chars().count(), 99);
        assert!(label.starts_with("4:33: あ"), "{label}");
        assert_eq!(value, "https://youtu.be/x");
    }

    /// No `00:00:00` for a length nobody knew.
    #[test]
    fn a_search_option_of_unknown_length_is_just_its_title() {
        let hit = AuxMetadata {
            title: Some("song".into()),
            ..Default::default()
        };
        assert_eq!(search_option(&hit).0, "song");
    }

    /// Discord rejects an empty option label, and the whole menu with it.
    #[test]
    fn a_search_option_with_no_title_and_no_length_is_untitled() {
        let hit = AuxMetadata::default();
        assert_eq!(search_option(&hit).0, "(untitled)");
        let blank = AuxMetadata {
            title: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(search_option(&blank).0, "(untitled)");
    }
}

#[cfg(test)]
mod test {

    use ::serenity::{
        all::Button,
        builder::{CreateActionRow, CreateComponent},
    };

    use crate::messaging::interface::create_single_nav_btn;
    use crack_types::to_fixed;

    use super::*;
    use crack_types::get_human_readable_timestamp;

    /// A playlist track with nothing known about it has a plain `(untitled)`
    /// line: no empty link, no 00:00.
    #[tokio::test]
    async fn a_playlist_page_line_without_link_or_duration_is_plain() {
        let blank = NewAuxMetadata(Default::default());
        let page = build_queue_page_metadata(&[blank], 0).await;
        assert_eq!(page, "`1.` **(untitled)**\n");
    }

    #[test]
    fn newline_splitter_never_slices_inside_a_multibyte_char() {
        let content = "a·—\n".repeat(40);
        for size in 1..20 {
            let chunks = split_string_into_chunks_newline(&content, size);
            assert_eq!(
                chunks.concat().replace('\n', ""),
                content.replace('\n', ""),
                "size {size}"
            );
        }
        // No newline in the window at all, boundary mid-char.
        let flat = "·—".repeat(30);
        for size in 1..10 {
            let chunks = split_string_into_chunks_newline(&flat, size);
            assert_eq!(chunks.concat(), flat, "size {size}");
        }
    }

    #[test]
    fn the_final_window_is_not_cut_at_its_last_newline() {
        // Everything fits: one page holding both lines.
        let pages = build_pages("a\nb", 900, false);
        assert_eq!(pages, vec!["a\nb".to_string()]);
        // Several pages: the last one keeps every line that fits, not just one.
        let lines: Vec<String> = (0..9).map(|i| format!("line{i}")).collect();
        let pages = build_pages(&lines.join("\n"), 20, false);
        let last = pages.last().unwrap();
        assert!(pages.len() > 1, "{pages:?}");
        assert!(last.contains("line7\nline8"), "{pages:?}");
        assert_eq!(pages.join("\n"), lines.join("\n"));
    }

    #[test]
    fn unfenced_pages_are_verbatim_and_fenced_pages_are_wrapped() {
        let line = "<t:1:R> · <@1> — x";
        let plain = build_pages(line, 900, false);
        assert_eq!(plain, vec![line.to_string()]);
        assert!(!plain[0].contains("```"));
        let fenced = build_pages(line, 900, true);
        assert_eq!(fenced, vec![format!("```md\n{line}\n```")]);
        assert_eq!(
            PagedStyle::default(),
            PagedStyle {
                ephemeral: false,
                fenced: true
            }
        );
    }

    #[test]
    fn page_count_is_the_chunk_count() {
        let content = "aaaaaa\n".repeat(10);
        assert_eq!(content.len() / 10 + 1, 8);
        assert_eq!(build_pages(&content, 10, false).len(), 10);
        assert_eq!(build_pages("", 10, true).len(), 1);
    }

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
        let (text, icon_url, vanity) = build_footer_info("https://www.rust-lang.org/");
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

        assert_eq!(btn.label, Some(to_fixed("<<" as &str)));
        assert!(btn.disabled);
    }

    #[test]
    fn test_build_nav_btns() {
        let nav_btns_vev = create_nav_btns(0, 1);
        if let CreateComponent::ActionRow(CreateActionRow::Buttons(nav_btns)) = &nav_btns_vev[0] {
            let mut btns = Vec::new();
            for btn in nav_btns.iter() {
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
            panic!("create_nav_btns did not return an action row of buttons");
        }
    }
}
