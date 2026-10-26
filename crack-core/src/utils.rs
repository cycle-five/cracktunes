use crate::http_utils::CacheHttpExt;
use crate::messaging::courier::{self, Destination};
use crate::messaging::discord::ToDiscord;
use crate::messaging::format::{duration_text, TrackLabel, INLINE_TITLE_MAX};
use crate::messaging::messages::{SEARCH_RESULTS_NOT_POSTED, TRACK_UNTITLED};
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
        messages::{PLAYLISTS, PLAYLIST_EMPTY, PLAYLIST_LIST_EMPTY, QUEUE_PAGE, QUEUE_PAGE_OF},
    },
    Context as CrackContext, CrackedError, CrackedResult, Data, Error,
};
use ::serenity::small_fixed_array::FixedString;
use ::serenity::{
    all::{CacheHttp, ComponentInteractionDataKind, GenericChannelId, GuildId, Interaction},
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    futures::StreamExt,
    model::channel::Message,
};
use anyhow::Result;
use crack_model::{Author, Card, Field, Footer, Row, Select, SelectOption};
use crack_types::NewAuxMetadata;
use crack_types::QueryType;
use poise::{
    serenity_prelude::{self as serenity, Context as SerenityContext},
    ReplyHandle,
};
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

pub const EMBED_PAGE_SIZE: usize = 6;
pub use crate::music::track_data::MUSIC_SEARCH_SUFFIX;

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
pub async fn send_reply<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    message: CrackedMessage,
    as_embed: bool,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    ctx.send_reply(message, as_embed).await
}

/// Sends a reply response, possibly as an embed.
pub async fn send_reply_owned(
    ctx: CrackContext<'_>,
    message: CrackedMessage,
    as_embed: bool,
) -> Result<ReplyHandle<'_>, CrackedError> {
    ctx.send_reply_owned(message, as_embed).await
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

/// The search results menu: one option per hit, the label first and the link
/// as the value.
fn song_select_rows(options: impl Iterator<Item = (String, String)>) -> Vec<Row> {
    vec![Row::Select(Select {
        id: "song_select".to_owned(),
        placeholder: Some("Select Song to Play".to_owned()),
        options: options
            .map(|(label, value)| SelectOption {
                label,
                value,
                default: false,
            })
            .collect(),
        min: None,
        max: None,
    })]
}

/// The page a navigation button leads to from `current` (counting from 0), or
/// `None` for a button that is not navigation.
fn flip_page(button: &str, current: usize, num_pages: usize) -> Option<usize> {
    match button {
        "<<" => Some(0),
        "<" => Some(min(current.saturating_sub(1), num_pages - 1)),
        ">" => Some(min(current.add(1), num_pages - 1)),
        ">>" => Some(num_pages - 1),
        _ => None,
    }
}

/// One page of a paged embed; `page` counts from 0, the footer from 1.
fn paged_card(
    title: &str,
    author: &str,
    description: String,
    page: usize,
    num_pages: usize,
) -> Card {
    Card {
        title: Some(title.to_owned()),
        author: Some(Author {
            name: author.to_owned(),
            url: None,
            icon_url: None,
        }),
        description: Some(description),
        footer: Some(Footer {
            text: format!("Page {}/{}", page + 1, num_pages),
            icon_url: None,
        }),
        ..Card::default()
    }
}

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
    let out = Rendered::text("Search results").with_rows(song_select_rows(res));
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
    #[expect(
        clippy::disallowed_methods,
        reason = "component responses move in PR 2"
    )]
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

    clear_menu_keeping(res, &transport, channel_id, menu_id).await
}

/// Delete the search menu and hand back `pick` either way: a menu left behind
/// is untidy, but the member's choice still plays. A failed delete used to
/// replace a successful pick with an error, and nothing was queued.
async fn clear_menu_keeping(
    pick: Result<QueryType, Error>,
    transport: &dyn Transport,
    channel: GenericChannelId,
    menu: ::serenity::all::MessageId,
) -> Result<QueryType, Error> {
    if let Err(err) = transport.delete(channel, menu).await {
        tracing::warn!("search: the results menu was not removed: {err:?}");
    }
    pick
}

/// Sends a reply response with an embed.
pub async fn send_embed_response_poise<'ctx>(
    ctx: CrackContext<'ctx>,
    card: Card,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    send_embed_response_poise_as(ctx, card, false).await
}

/// [`send_embed_response_poise`], ephemeral when `ephemeral` is set.
pub async fn send_embed_response_poise_as<'ctx>(
    ctx: CrackContext<'ctx>,
    card: Card,
    ephemeral: bool,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    courier::reply_rendered(ctx, Rendered::card(card), ephemeral).await
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
pub async fn edit_embed_response2(
    ctx: CrackContext<'_>,
    card: Card,
    msg: ReplyHandle<'_>,
    content: Option<String>,
) -> Result<(), Error> {
    let mut out = Rendered::card(card);
    if let Some(content) = content {
        out = out.with_content(content);
    }
    courier::edit_rendered(ctx, &msg, out).await?;
    Ok(())
}

pub(crate) use crate::music::track_data::track_data;
pub use crate::music::track_data::{get_requesting_user, get_track_handle_metadata, TrackData};

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

pub async fn build_playlist_list_embed(playlists: &[Playlist], page: usize) -> Card {
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

    Card {
        title: Some(PLAYLISTS.to_owned()),
        description: Some(content),
        ..Card::default()
    }
}

pub async fn build_tracks_embed_metadata(
    playlist_name: String,
    metadata_arr: &[NewAuxMetadata],
    page: usize,
) -> Card {
    Card {
        fields: vec![Field {
            name: playlist_name,
            value: build_queue_page_metadata(metadata_arr, page).await,
            inline: false,
        }],
        footer: Some(Footer {
            text: format!(
                "{} {} {} {}",
                QUEUE_PAGE,
                page + 1,
                QUEUE_PAGE_OF,
                calculate_num_pages(metadata_arr),
            ),
            icon_url: None,
        }),
        ..Card::default()
    }
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
                Rendered::card(paged_card(&title, &author, page_getter(0), 0, num_pages))
                    .with_rows(create_nav_btns(0, num_pages)),
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

            *page_wlock = match flip_page(btn_id, *page_wlock, num_pages) {
                Some(next) => next,
                None => continue,
            };

            let flipped = Rendered::card(paged_card(
                &title,
                &author,
                page_getter(*page_wlock),
                *page_wlock,
                num_pages,
            ))
            .with_rows(create_nav_btns(*page_wlock, num_pages));
            #[expect(
                clippy::disallowed_methods,
                reason = "component responses move in PR 2"
            )]
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

pub use crate::messaging::cards::build_footer_info;
pub use crate::messaging::format::duration_to_string;

use serenity::prelude::SerenityError;

/// Check if a subdomian is from the same domain.
pub fn compare_domains(domain: &str, subdomain: &str) -> bool {
    subdomain == domain || subdomain.ends_with(domain)
}

/// Takes a Result ReplyHandle and logs the error if it's an Err.
pub fn check_reply(result: Result<ReplyHandle, SerenityError>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
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
    use crate::messaging::discord;

    /// A pick stands when its menu cannot be deleted: the delete failure is
    /// logged, and the pick is what comes back.
    #[tokio::test]
    async fn a_menu_that_will_not_delete_does_not_cost_the_pick() {
        use crate::messaging::test_support::{FakeTransport, Op};
        use crate::messaging::transport::TransportError;
        use ::serenity::all::{GenericChannelId, MessageId};

        let t = FakeTransport::default();
        *t.delete_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        let pick = Ok(QueryType::VideoLink("https://youtu.be/x".into()));
        let got =
            super::clear_menu_keeping(pick, &t, GenericChannelId::new(7), MessageId::new(42)).await;
        assert!(
            matches!(&got, Ok(QueryType::VideoLink(url)) if url == "https://youtu.be/x"),
            "{got:?}"
        );
        assert_eq!(t.ops(), vec![Op::Delete(7, 42)]);
    }

    use crate::messaging::interface::create_single_nav_btn;

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
        let btn = create_single_nav_btn("<<", true);

        assert_eq!(btn.label.as_deref(), Some("<<"));
        assert!(btn.disabled);
    }

    #[test]
    fn test_build_nav_btns() {
        let nav_btns_vev = create_nav_btns(0, 1);
        if let Row::Buttons(btns) = &nav_btns_vev[0] {
            assert_eq!(btns.len(), 4);
        } else {
            panic!("create_nav_btns did not return an action row of buttons");
        }
    }

    fn sample_playlists() -> Vec<Playlist> {
        vec![
            Playlist {
                id: 7,
                name: "mix".into(),
                ..Default::default()
            },
            Playlist {
                id: 9,
                name: "two".into(),
                ..Default::default()
            },
        ]
    }

    fn sample_tracks() -> Vec<NewAuxMetadata> {
        vec![NewAuxMetadata(AuxMetadata {
            title: Some("Song".into()),
            source_url: Some("https://youtu.be/x".into()),
            duration: Some(Duration::from_secs(61)),
            ..Default::default()
        })]
    }

    #[test]
    fn song_select_menu_renders_as_before() {
        let options = vec![
            ("a".to_string(), "https://x/a".to_string()),
            ("b".to_string(), "https://x/b".to_string()),
        ];
        let rows = song_select_rows(options.into_iter());
        assert_eq!(
            serde_json::to_string(&discord::action_rows(&rows)).unwrap(),
            r#"[{"type":1,"components":[{"custom_id":"song_select","placeholder":"Select Song to Play","type":3,"options":[{"label":"a","value":"https://x/a"},{"label":"b","value":"https://x/b"}]}]}]"#
        );
    }

    #[test]
    fn navigation_buttons_flip_pages_within_bounds() {
        assert_eq!(flip_page("<<", 3, 5), Some(0));
        assert_eq!(flip_page("<", 3, 5), Some(2));
        assert_eq!(flip_page("<", 0, 5), Some(0));
        assert_eq!(flip_page("<", 9, 5), Some(4));
        assert_eq!(flip_page(">", 3, 5), Some(4));
        assert_eq!(flip_page(">", 0, 5), Some(1));
        assert_eq!(flip_page(">", 4, 5), Some(4));
        assert_eq!(flip_page(">>", 1, 5), Some(4));
        assert_eq!(flip_page("x", 1, 5), None);
    }

    #[test]
    fn paged_embed_renders_as_before() {
        let first = paged_card("T", "Au", "desc".to_string(), 0, 5);
        assert_eq!(
            serde_json::to_string(&discord::embed(&first)).unwrap(),
            r#"{"title":"T","type":"rich","description":"desc","footer":{"text":"Page 1/5","icon_url":null},"author":{"name":"Au","url":null,"icon_url":null}}"#
        );
        let card = paged_card("T", "Au", "desc".to_string(), 1, 5);
        assert_eq!(
            serde_json::to_string(&discord::embed(&card)).unwrap(),
            r#"{"title":"T","type":"rich","description":"desc","footer":{"text":"Page 2/5","icon_url":null},"author":{"name":"Au","url":null,"icon_url":null}}"#
        );
    }

    #[tokio::test]
    async fn an_empty_playlist_list_renders_as_before() {
        let card = build_playlist_list_embed(&[], 0).await;
        assert_eq!(
            serde_json::to_string(&discord::embed(&card)).unwrap(),
            "{\"title\":\"Playlists\",\"type\":\"rich\",\"description\":\"📃\u{2002}You have no playlists currently.\"}"
        );
    }

    #[tokio::test]
    async fn a_playlist_list_renders_as_before() {
        let card = build_playlist_list_embed(&sample_playlists(), 0).await;
        assert_eq!(
            serde_json::to_string(&discord::embed(&card)).unwrap(),
            r#"{"title":"Playlists","type":"rich","description":"`1.` mix (7)\n`2.` two (9)\n"}"#
        );
    }

    #[tokio::test]
    async fn a_playlist_page_renders_as_before() {
        let card = build_tracks_embed_metadata("My list".into(), &sample_tracks(), 0).await;
        assert_eq!(
            serde_json::to_string(&discord::embed(&card)).unwrap(),
            r#"{"type":"rich","footer":{"text":"Page 1 of 1","icon_url":null},"fields":[{"name":"My list","value":"`1.` [**Song**](https://youtu.be/x) • `1:01`\n","inline":false}]}"#
        );
        let card = build_tracks_embed_metadata("My list".into(), &sample_tracks(), 3).await;
        assert_eq!(
            serde_json::to_string(&discord::embed(&card)).unwrap(),
            "{\"type\":\"rich\",\"footer\":{\"text\":\"Page 4 of 1\",\"icon_url\":null},\"fields\":[{\"name\":\"My list\",\"value\":\"📃\u{2002}This playlist has no songs!\",\"inline\":false}]}"
        );
    }
}
