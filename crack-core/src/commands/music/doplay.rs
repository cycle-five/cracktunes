// Nightly reports `unreachable_code` inside `poise::command` (stable does not).
// An item-level allow does not cover a lint whose span is the attribute macro.
#![allow(unreachable_code)]

use crate::commands::get_call_or_join_author;
use crate::commands::{cmd_check_music, help};
use crate::music::query::{query_type_from_url, ResolvedQuery};
use crate::music::queue::{get_mode, get_msg, queue_track_back};
use crate::music::NewQueryType;
use crate::CrackedResult;
use crate::{
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    handlers::track_end::update_queue_messages,
    messaging::cards::QueuedCard,
    messaging::courier,
    messaging::format::{duration_text, escape, http_url, TrackLabel},
    messaging::interface::now_playing_card,
    messaging::messages::TRACK_UNTITLED,
    messaging::placeholder::{discard_on_err, Placeholder},
    messaging::render::{render, RenderCx, Rendered},
    messaging::transport::DiscordTransport,
    messaging::{
        message::CrackedMessage,
        messages::{PLAY_QUEUE, PLAY_TOP},
    },
    music::query::ListingShortfall,
    poise_ext::ContextExt,
    sources::youtube::build_query_aux_metadata,
    utils::get_track_handle_metadata,
    Context, Data, Error,
};
use ::serenity::all::CreateAutocompleteResponse;
use ::serenity::{
    all::{CommandInteraction, Message},
    builder::{CreateEmbed, CreateEmbedFooter},
};
use crack_types::QueryType;
use crack_types::{search_result_to_aux_metadata, Mode, NewAuxMetadata};
use poise::{serenity_prelude as serenity, ReplyHandle};
use songbird::{tracks::TrackHandle, Call};
use std::borrow::Cow;
use std::{cmp::Ordering, sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// Get the guild name.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    let shard_id = ctx.serenity_context().shard_id;
    // The guild's name is the guild's text, not ours: escaped.
    let name = escape(&ctx.partial_guild().await.unwrap().name);
    courier::reply(
        ctx,
        CrackedMessage::Other(format!(
            "The name of this guild is: {}, shard_id: {}",
            name, shard_id
        )),
    )
    .await?;

    Ok(())
}

/// Play a song next
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("next", "pn", "Pn", "insert", "ins", "push"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn playnext(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, Some("next".to_string()), None, query_or_url).await
}

/// Search interactively for a song
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("s", "S"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn search(
    ctx: Context<'_>,
    #[rest]
    #[description = "search query."]
    query: String,
) -> Result<(), Error> {
    play_internal(ctx, Some("search".to_string()), None, Some(query)).await
}

use crack_testing::suggestion2;

/// Autocomplete to suggest a search query.
pub async fn autocomplete<'a>(
    _ctx: poise::ApplicationContext<'_, Data, Error>,
    searching: &'a str,
) -> CreateAutocompleteResponse<'a> {
    // let choices = match suggestion2(searching).await {
    //     Ok(x) => {
    //         let choices = x.iter().map(|choice| choice).collect();
    //         choices
    //     },
    //     Err(e) => {
    //         tracing::error!("Error getting suggestions: {:?}", e);
    //         vec![]
    //     },
    // };
    let choices = suggestion2(searching).await.unwrap_or_default();
    let res = CreateAutocompleteResponse::new();
    res.set_choices(Cow::Owned(choices.clone()))
}

/// Play a song.
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("p", "P"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn play(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    #[autocomplete = "autocomplete"]
    query: String,
) -> Result<(), Error> {
    // Split off the first part of the query for
    let query = query.split("~").next().unwrap_or_default().to_string();
    play_internal(ctx, None, None, Some(query)).await
}

/// Play a song with more options
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    aliases("opt"),
    check = "cmd_check_music"
)]
pub async fn optplay(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
    #[description = "Play mode"] mode: Option<String>,
    #[description = "File to play."] file: Option<serenity::Attachment>,
    #[description = "song link or search query."] query_or_url: Option<String>,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    play_internal(ctx, mode, file, query_or_url).await
}

/// Play a local file.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    category = "Music",
    check = "cmd_check_music"
)]
pub async fn playfile(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
    #[description = "File to play."] file: serenity::Attachment,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    play_internal(ctx, None, Some(file), None).await
}

// `enqueue_resolved_tracks` (called `handler.enqueue_input(...)` directly,
// bypassing the guard funnel) was removed here: both its call sites were
// already commented out (below, and queue.rs's `queue_query_list_offset`),
// and being `pub` it never tripped `dead_code`, so it sat as an unguarded
// public escape hatch of exactly the class this funnel exists to close. Use
// `enqueue_resolved_tracks_back` instead if this is ever needed again.

// /// Pushes a track to the front of the queue, after readying it.
// pub async fn queue_track_ready_front(
//     call: &Arc<Mutex<Call>>,
//     ready_track: TrackReadyData,
// ) -> Result<Vec<TrackHandle>, CrackedError> {
//     let mut handler = call.lock().await;
//     let track_handle = handler.enqueue_input(ready_track.source).await;
//     let new_q = handler.queue().current_queue();
//     // Zeroth index: Currently playing track
//     // First index: Current next track
//     // Second index onward: Tracks to be played, we get in here most likely,
//     // but if we're in one of the first two we don't want to do anything.
//     if new_q.len() >= 3 {
//         //return Ok(new_q);
//         handler.queue().modify_queue(|queue| {
//             let back = queue.pop_back().unwrap();
//             queue.insert(1, back);
//         });
//     }

//     drop(handler);
//     let mut map = track_handle.typemap().write().await;
//     map.insert::<NewAuxMetadata>(ready_track.metadata.clone());
//     map.insert::<RequestingUser>(RequestingUser::UserId(
//         ready_track.user_id.unwrap_or(UserId::new(1)),
//     ));
//     drop(map);
//     Ok(new_q)
// }

/// Play a youtube playlist.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument(skip(ctx))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    category = "Music",
    check = "cmd_check_music"
)]
pub async fn playytplaylist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist URL."]
    query: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // Refused before joining voice: only a YouTube playlist link is fetched.
    let query =
        crack_types::canonical_youtube_playlist_url(&query).ok_or(CrackedError::InvalidPlaylist)?;
    let crack_client = ctx.data().ct_client.clone();
    // This retrieves the call that the bot is connected to or joins the author's channel.
    // We error hear if the bot can't join the channel, or if the author isn't in a channel,
    // or the bot is in another channel, etc. So this should happen first.
    let _call = get_call_or_join_author(ctx).await?;
    // This gets the metadata for all the tracks in the playlist.
    // At this point we should have enough information to determine if any of the tracks
    // aren't allowed or able to be played (possibly?) and display the who list of them.
    let _tracks = crack_client.resolve_playlist(&query).await?;
    let queued = crack_client.get_queue(guild_id).await;
    let yt_playlist_str = playlist_display(
        queued
            .iter()
            .map(|t| (t.get_title(), t.get_url(), t.duration())),
    );
    tracing::warn!("yt_playlist_str: {}", yt_playlist_str);
    courier::reply(ctx, CrackedMessage::Other(yt_playlist_str)).await?;
    // This enqueues the tracks into the internal queue for the bot.
    //let _ = enqueue_resolved_tracks(call, tracks).await;
    courier::reply(ctx, CrackedMessage::PlaylistQueued).await?;
    Ok(())
}

/// The "Queuing..." progress line. The query is built from third-party
/// metadata, so it is escaped.
fn queuing_text(query: &str) -> String {
    format!("Queuing... {}", escape(query))
}

/// One playlist entry as `[title](url) • `3:21``. The title is third-party
/// text and is escaped; a URL that is not http(s) is dropped rather than
/// linked, and parentheses in it cannot end the link early. The length reads
/// as everywhere else (`m:ss`), and is left out when unknown, never `00:00`.
fn playlist_line(title: &str, url: &str, duration: Option<Duration>) -> String {
    let title = match title.trim() {
        "" => TRACK_UNTITLED.to_owned(),
        t => escape(t),
    };
    let length = duration_text(duration)
        .map(|d| format!(" • `{d}`"))
        .unwrap_or_default();
    match http_url(Some(url)) {
        Some(url) => {
            let target = url.as_str().replace('(', "%28").replace(')', "%29");
            format!("[{title}]({target}){length}")
        },
        None => format!("{title}{length}"),
    }
}

/// The playlist display: one [`playlist_line`] per `(title, url, duration)`.
fn playlist_display(entries: impl Iterator<Item = (String, String, Option<Duration>)>) -> String {
    entries
        .map(|(title, url, duration)| playlist_line(&title, &url, duration))
        .collect::<Vec<_>>()
        .join("\n")
}

use crate::commands::resume_internal;
use crate::messaging::interface as msg_int;
use crate::music::perms::TextPerms;
use crate::poise_ext::PoiseContextExt;
use crack_types::to_fixed;

/// The note appended to a play reply when the bot cannot fully use the text
/// channel. `None` means say nothing — either everything is granted, or the
/// cache could not tell us, and a false warning is worse than silence.
///
/// This is a note on a reply that was being sent anyway, never a message of
/// its own: it costs nothing extra, stays next to the action that prompted
/// it, and disappears the moment the permission is granted, with no state to
/// keep and nothing to expire.
fn degraded_notice(text: Option<&TextPerms>) -> Option<String> {
    let text = text?;
    if text.is_whole() {
        return None;
    }
    Some(format!(
        "⚠️ Missing **{}** here — I won't post now-playing. `/diagnose` for detail.",
        text.missing()
    ))
}

/// Where the note has to ride to actually be seen.
///
/// 🪤 An embed field is invisible in exactly the case that needs it most. On a
/// prefix `r!play` in a channel without `EMBED_LINKS`, Discord strips the
/// embed from the message — and takes "Missing **Embed Links** here" with it.
/// The one reader who needs that sentence is the only one who never gets it.
/// Message content is not stripped, so that is where the note goes whenever
/// `EMBED_LINKS` is among the missing set.
///
/// Slash commands are not affected (interaction responses bypass the
/// channel's permission check), but the note is routed the same way for both:
/// one rule, and the surviving delivery is correct everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NoticeDelivery {
    /// Embeds render here, so the note rides as a field on the reply's embed,
    /// next to the queue entry it qualifies.
    Field(String),
    /// `EMBED_LINKS` is missing, so the embed may never be shown. The note
    /// goes in the message content, which always survives.
    Content(String),
}

/// Route [`degraded_notice`] to a delivery that will actually be seen.
fn degraded_delivery(text: Option<&TextPerms>) -> Option<NoticeDelivery> {
    let note = degraded_notice(text)?;
    // `text` is Some here: degraded_notice returned a note.
    if text.is_some_and(|t| !t.embed()) {
        Some(NoticeDelivery::Content(note))
    } else {
        Some(NoticeDelivery::Field(note))
    }
}

/// The play reply for a track that is playing now.
async fn now_playing(track: &TrackHandle) -> CrackedMessage {
    CrackedMessage::NowPlayingCard(Box::new(now_playing_card(track).await))
}

/// The play reply for `track`, just queued with `wait` ahead of it.
async fn queued(
    author: &'static str,
    track: &TrackHandle,
    wait: Option<Duration>,
) -> CrackedMessage {
    let metadata = get_track_handle_metadata(track).await.unwrap_or_default();
    CrackedMessage::Queued(Box::new(QueuedCard {
        author,
        label: TrackLabel::from_metadata(&metadata),
        thumbnail: metadata.thumbnail,
        wait,
    }))
}

/// What the play reply says: the track queued and when it plays, the
/// playlist queued (in words, never its variant name), or what is playing.
pub async fn build_play_message(
    queue: &[TrackHandle],
    mode: Mode,
    query_type: NewQueryType,
) -> CrackedMessage {
    match queue.len().cmp(&1) {
        Ordering::Greater => {
            let NewQueryType(query_type) = query_type;
            match (query_type, mode) {
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::Next,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::Next");
                    let wait = calculate_time_until_play(queue, mode).await;
                    queued(PLAY_TOP, &queue[1], wait).await
                },
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::End,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::End");
                    let wait = calculate_time_until_play(queue, mode).await;
                    queued(PLAY_QUEUE, &queue[queue.len() - 1], wait).await
                },
                (QueryType::PlaylistLink(_) | QueryType::KeywordList(_), y) => {
                    tracing::error!(
                        "QueryType::PlaylistLink|QueryType::KeywordList, mode: {:?}",
                        y
                    );
                    CrackedMessage::PlaylistQueued
                },
                (QueryType::File(_x_), y) => {
                    tracing::error!("QueryType::File, mode: {:?}", y);
                    now_playing(&queue[0]).await
                },
                (QueryType::YoutubeSearch(_x), y) => {
                    tracing::error!("QueryType::YoutubeSearch, mode: {:?}", y);
                    now_playing(&queue[0]).await
                },
                (x, y) => {
                    tracing::error!("{:?} {:?} {:?}", x, y, mode);
                    now_playing(&queue[0]).await
                },
            }
        },
        Ordering::Equal => {
            tracing::warn!("Only one track in queue, just playing it.");
            now_playing(&queue[0]).await
        },
        Ordering::Less => {
            tracing::warn!("No tracks in queue, this only happens when an interactive search is done with an empty queue.");
            CrackedMessage::CreateEmbed(Box::new(
                CreateEmbed::default()
                    .description("No tracks in queue!")
                    .footer(CreateEmbedFooter::new("No tracks in queue!")),
            ))
        },
    }
}

/// The whole play reply: the message, plus the permission note where it will
/// actually be seen.
///
/// 🪤 One [`Rendered`], content and embed together, on purpose. The
/// `Content` note exists precisely because a channel without `EMBED_LINKS`
/// never shows the embed; carrying both in one value means no caller can
/// deliver the embed and drop the note.
pub async fn build_play_reply(
    queue: &[TrackHandle],
    mode: Mode,
    query_type: NewQueryType,
    text: Option<&TextPerms>,
) -> Rendered {
    let msg = build_play_message(queue, mode, query_type).await;
    with_notice(render(&msg, &RenderCx::now()), degraded_delivery(text))
}

/// `out` with the permission note added where [`NoticeDelivery`] says.
fn with_notice(mut out: Rendered, notice: Option<NoticeDelivery>) -> Rendered {
    match notice {
        Some(NoticeDelivery::Field(note)) => match out.embed.take() {
            Some(embed) => {
                out.embed = Some(embed.field("⚠️ Limited permissions", note, false));
                out
            },
            // Every play reply is an embed; were one not, the note still rides.
            None => out.with_content(note),
        },
        // The embed is kept for whoever can see it; the note rides outside
        // it, because Discord strips the embed in exactly this channel.
        Some(NoticeDelivery::Content(note)) => out.with_content(note),
        None => out,
    }
}

/// What `/play` knows once its placeholder has become the reply.
struct FilledReply {
    shortfall: Option<ListingShortfall>,
    was_empty: bool,
    queue_len: usize,
    after_query_type: std::time::Instant,
    after_move_on: std::time::Instant,
    after_refetch_queue: std::time::Instant,
    after_embed: std::time::Instant,
}

/// Everything fallible between sending `/play`'s placeholder and editing it
/// into the reply (#494). 🔑 It ends AT the edit: once that succeeds the
/// placeholder is the reply, and nothing after it may take it down.
async fn fill_search_reply(
    ctx: Context<'_>,
    url: &str,
    file: Option<serenity::Attachment>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    search_msg: &ReplyHandle<'_>,
) -> Result<FilledReply, Error> {
    // determine whether this is a link or a query string
    let query_type = query_type_from_url(ctx, url, file).await?;

    // FIXME: Decide whether we're using this everywhere, or not.
    // Don't like the inconsistency.
    let resolved = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;
    // The shortfall travels beside the query so it can be reported *after* the
    // queue reply, once there is something to report it against.
    let ResolvedQuery {
        query: query_type,
        shortfall,
    } = resolved;

    tracing::warn!("query_type: {:?}", query_type);

    let after_query_type = std::time::Instant::now();

    // Read before anything below enqueues into it, so a playlist (or any
    // multi-track result) landing in an idle bot is still recognized as
    // having started a song -- not just a `/play` that queued exactly one.
    // And no earlier: resolving the query above takes seconds, and a track
    // that ended in that window would leave the new song under "Finished".
    // 🔑 The Call guard is a temporary, released at the end of the statement.
    let was_empty = call.lock().await.queue().is_empty();

    // FIXME: Super hacky, fix this shit.
    // This is actually where the track gets queued into the internal queue, it's the main work function.
    match_mode(
        ctx,
        call.clone(),
        mode,
        query_type.clone(),
        search_msg.clone(),
    )
    .await?;

    let after_move_on = std::time::Instant::now();

    // refetch the queue after modification
    // FIXME: I'm beginning to think that this walking of the queue is what's causing the performance issues.
    // let handler = call.lock().await;
    // let queue = handler.queue().current_queue();
    // drop(handler);
    let queue = call.lock().await.queue().current_queue();

    let after_refetch_queue = std::time::Instant::now();

    // This makes sense, we're getting the final response to the user based on whether
    // the song / playlist was queued first, last, or is now playing.
    // Ah! Also, sometimes after a long queue process the now playing message says that it's already
    // X seconds into the song, so this is definitely after the section of the code that
    // takes a long time.
    let text_perms = ctx.guild_id().and_then(|gid| {
        crate::music::perms::resolve(ctx.cache(), gid, ctx.channel_id(), ctx.author().id)
            .map(|p| p.text)
    });
    let reply = build_play_reply(&queue, mode, query_type, text_perms.as_ref()).await;

    let after_embed = std::time::Instant::now();

    courier::edit_rendered(ctx, search_msg, reply).await?;

    Ok(FilledReply {
        shortfall,
        was_empty,
        queue_len: queue.len(),
        after_query_type,
        after_move_on,
        after_refetch_queue,
        after_embed,
    })
}

/// Does the actual playing of the song, all the other commands use this.
//#[tracing::instrument(skip(ctx))]
#[cfg(not(tarpaulin_include))]
pub async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // FIXME: This should be generalized.
    // Get current time for timing purposes.

    let _start = std::time::Instant::now();

    let is_prefix = ctx.is_prefix();

    let msg = get_msg(mode.clone(), query_or_url, is_prefix).map(to_fixed);

    if msg.is_none() && file.is_none() {
        if ctx.is_paused().await.unwrap_or_default() {
            return resume_internal(ctx).await;
        }
        // An error, so red, as every rendered error is.
        courier::reply(ctx, CrackedMessage::CrackedError(CrackedError::NoQuery)).await?;
        return Ok(());
    }

    let _after_msg_parse = std::time::Instant::now();

    let mode = mode.map(to_fixed);
    let (mode, msg) = get_mode(is_prefix, msg.clone(), mode);

    let _after_get_mode = std::time::Instant::now();

    // TODO: Maybe put into it's own function?
    let url = match file.clone() {
        Some(file) => file.url.clone(),
        None => msg.clone(),
    };
    let url = url.as_str();

    tracing::warn!(target: "PLAY", "url: {}", url);

    let call = get_call_or_join_author(ctx).await?;

    let _after_call = std::time::Instant::now();

    // `ephemeral_replies` decides whether this reply -- and the edit that turns
    // it into the result -- is seen by its author alone.
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let private = crate::messaging::status::reply_privately(
        ctx.data().get_ephemeral_replies(guild_id).await,
        is_prefix,
    );
    let search_msg = msg_int::send_search_message_as(&ctx, private).await?;
    //tracing::debug!("search response msg: {:?}", search_msg.message());

    // 🔑 #494: nothing fallible between this send and `discard_on_err`.
    // Everything that can fail before the placeholder becomes the reply lives
    // in `fill_search_reply`, and its error takes the placeholder down with it.
    let FilledReply {
        shortfall,
        was_empty,
        queue_len,
        after_query_type: _after_query_type,
        after_move_on: _after_move_on,
        after_refetch_queue: _after_refetch_queue,
        after_embed: _after_embed,
    } = discard_on_err(
        &Placeholder {
            ctx,
            handle: &search_msg,
        },
        fill_search_reply(ctx, url, file, call.clone(), mode, &search_msg).await,
    )
    .await?;

    // A partial listing is a success with something missing, so it is said
    // after the queue embed rather than instead of it: the recovered tracks are
    // already queued and playing, and this is the footnote. Silent when the
    // listing was whole, which is the overwhelming majority of the time.
    let mut footnote = None;
    if let Some(short) = shortfall {
        tracing::warn!(
            "spotify: partial listing served -- {} of {} seen, {} missing",
            short.seen,
            short.declared,
            short.missing
        );
        // The guild's ephemeral choice: the footnote to an ephemeral result
        // must not land as a visible message.
        let msg = CrackedMessage::SpotifyListingShort {
            seen: short.seen,
            declared: short.declared,
            missing: short.missing,
        };
        footnote = Some(courier::reply_as(ctx, msg, private).await?);
    }

    // A `/play` that started a song is a now-playing moment. The status follows
    // the reply, so a visible reply ends up directly above it.
    if crate::messaging::status::play_started_song(was_empty, queue_len) {
        // The floor is the newest visible reply -- the footnote if one was
        // sent, else the search reply edited into the result -- because its
        // gateway echo may not have reached the cache yet. An ephemeral reply
        // is not a channel message and must not move the status.
        let after = if private {
            None
        } else {
            crate::messaging::courier::locate(ctx, footnote.as_ref().unwrap_or(&search_msg)).await
        };
        let serenity_ctx = ctx.serenity_context();
        crate::messaging::status::show_now_playing_after(
            &ctx.data(),
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
            after,
        )
        .await;
    }

    // [Manage Messages]: Permissions::MANAGE_MESSAGES
    // I think this does different things based on prefix or not?
    // if !is_prefix {
    //     match search_msg.delete(&ctx).await {
    //         Ok(_) => {},
    //         Err(e) => {
    //             tracing::error!("Error deleting search message: {:?}", e);
    //         },
    //     }
    // }

    let _after_edit_embed = std::time::Instant::now();

    tracing::warn!(
        r#"
        after_msg_parse: {:?}
        after_get_mode: {:?} (+{:?})
        after_call: {:?} (+{:?})
        after_query_type: {:?} (+{:?})
        after_move_on: {:?} (+{:?})
        after_refetch_queue: {:?} (+{:?})
        after_embed: {:?} (+{:?})
        after_edit_embed: {:?} (+{:?})"#,
        _after_msg_parse.duration_since(_start),
        _after_get_mode.duration_since(_start),
        _after_get_mode.duration_since(_after_msg_parse),
        _after_call.duration_since(_start),
        _after_call.duration_since(_after_get_mode),
        _after_query_type.duration_since(_start),
        _after_query_type.duration_since(_after_call),
        _after_move_on.duration_since(_start),
        _after_move_on.duration_since(_after_query_type),
        _after_refetch_queue.duration_since(_start),
        _after_refetch_queue.duration_since(_after_move_on),
        _after_embed.duration_since(_start),
        _after_embed.duration_since(_after_refetch_queue),
        _after_edit_embed.duration_since(_start),
        _after_edit_embed.duration_since(_after_embed),
    );
    Ok(())
}
pub enum MessageOrInteraction {
    Message(Message),
    Interaction(CommandInteraction),
}

pub async fn get_user_message_if_prefix(ctx: Context<'_>) -> MessageOrInteraction {
    match ctx {
        Context::Prefix(ctx) => MessageOrInteraction::Message(ctx.msg.clone()),
        Context::Application(ctx) => MessageOrInteraction::Interaction(ctx.interaction.clone()),
    }
}

/// This function takes a [`NewQueryType`] and resolves it to zero or more tracks in [`Vec<Track>`].
pub async fn resolve_query_to_tracks(
    ctx: Context<'_>,
    _call: Arc<Mutex<Call>>,
    query_type: NewQueryType,
) -> CrackedResult<Vec<crate::music::queue::QueuedTrack>> {
    let client = ctx.data().ct_client.clone();
    let NewQueryType(query_type) = query_type;
    let tracks = client.resolve_query_to_tracks(query_type.clone()).await?;
    //let tracks = client.resolve_track(query_type).await?;
    let mut track_handles = Vec::new();
    for track in tracks.iter() {
        // Through `build_track`, not a third copy of it -- see the note there.
        track_handles.push(crate::music::queue::build_track(track, &client.req_client)?);
    }
    Ok(track_handles)
}

// let resolved = match ctx.data().ct_client.resolve_track(query_type.clone()).await {
//         Ok(resolved) => resolved.with_user_id(user_id),
//         Err(e1) => {
//             match e1.into() {
//                 Some(_e) => {
//                     let ready_track = ready_query(ctx, query_type.clone()).await?;
//                     return _queue_track_ready_back(call, ready_track).await;
//                 },
//                 None => {
//                     return Err(CrackedError::TrackResolveError(
//                         TrackResolveError::UnknownQueryType,
//                     ));
//                 },
//             };
//         },
//     }
/// This is what actually does the majority of the work of the function.
/// It finds the track that the user wants to play and then actually
/// does the process of queuing it. This needs to be optimized.
async fn match_mode(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: NewQueryType,
    //search_msg: &'a mut ReplyHandle<'a>,
    search_msg: ReplyHandle<'_>,
) -> CrackedResult<()> {
    tracing::info!("mode: {:?}", mode);

    // ctx.data().ct_client.resolve_query(&query_type).await?;

    // let ctx = Arc::new(ctx.clone());
    match mode {
        Mode::Search => {
            let res = query_type.mode_search(ctx, call).await?;
            if res.is_empty() {
                Err(CrackedError::Other("No results found!"))
            } else {
                Ok(())
            }
        },
        Mode::DownloadMKV => query_type.mode_download(ctx, false).await.map(|_| ()),
        Mode::DownloadMP3 => query_type.mode_download(ctx, true).await.map(|_| ()),
        Mode::End => query_type
            .mode_end(ctx, call, search_msg.clone())
            .await
            .map(|_| ()),
        Mode::Next => query_type
            .mode_next(ctx, call, search_msg.clone())
            .await
            .map(|_| ()),
        Mode::Jump => query_type.mode_jump(ctx, call).await.map(|_| ()),
        Mode::All | Mode::Reverse | Mode::Shuffle => query_type
            .mode_rest(ctx, call, search_msg)
            .await
            .map(|_| ()),
    }
}

// /// new match_mode function.
// async fn match_mode_new<'ctx>(
//     ctx: Context<'_>,
//     call: Arc<Mutex<Call>>,
//     mode: Mode,
//     query_type: QueryType,
//     search_msg: Message,
// ) -> JoinHandle<dyn std::future::Future<Output = CrackedResult<bool>> + Send> {
//     tracing::info!("mode: {:?}", mode);

//     tokio::task::spawn(async move {
//         //let ctx = *ctx.as_ref().to_owned();
//         match mode {
//             _ => query_type.mode_download(ctx, false),
//         }
//     })
//     // let handle = tokio::spawn(async move {
//     //     let ctx2 = ctx.as_ref();
//     //     match mode {
//     //         _ => {
//     //             // query_type.mode_end(ctx, call, search_msg)
//     //             let beg_time = std::time::Instant::now();
//     //             let _ready_q = match query_type.get_track_source_and_metadata(None).await {
//     //                 Ok(x) => x,
//     //                 Err(e) => {
//     //                     return Err(e);
//     //                 },
//     //             };
//     //             let end_time = std::time::Instant::now();
//     //             tracing::info!(
//     //                 "get_track_source_and_metadata: {:?}",
//     //                 end_time.duration_since(beg_time)
//     //             );
//     //             let res = match query_type.mode_end(ctx, call, search_msg.clone()).await {
//     //                 Ok(x) if x => (),
//     //                 Ok(_) => {
//     //                     return Err(CrackedError::Other("No tracks in queue!"));
//     //                 },
//     //                 Err(e) => {
//     //                     return Err(e);
//     //                 },
//     //             };
//     //             Ok(())
//     //         },
//     //         // Mode::Search => query_type.mode_search(ctx1, call).await,
//     //         //    .map(|x| !x.is_empty()),
//     //         // Mode::DownloadMKV => query_type.mode_download(ctx, false).await,
//     //         // Mode::DownloadMP3 => query_type.mode_download(ctx, true).await,
//     //         // Mode::Next => query_type.mode_next(ctx, call, search_msg).await,
//     //         // Mode::Jump => query_type.mode_jump(ctx, call).await,
//     //         // Mode::All | Mode::Reverse | Mode::Shuffle => {
//     //         //     query_type.mode_rest(ctx, call, search_msg).await
//     //         // },
//     //         // _ => unimplemented!(),
//     //     }
//     //});

//     //handle
// }

// async fn query_type_to_metadata<'a>(
//     ctx: Context<'_>,
//     call: Arc<Mutex<Call>>,
//     mode: Mode,
//     query_type: QueryType,
//     search_msg: &'a mut Message,
// ) -> CrackedResult<bool> {
//     tracing::info!("mode: {:?}", mode);
// }

/// How long until the track just queued plays: what is left of the one
/// playing, plus the length of each track between them. `None` when any of
/// those lengths is unknown (a live stream, a track nothing measured), since
/// an estimate built on a guess is worse than none.
async fn calculate_time_until_play(queue: &[TrackHandle], mode: Mode) -> Option<Duration> {
    let playing = queue.first()?;
    // Bounded: a driver that does not answer reads as "just started".
    let elapsed =
        match tokio::time::timeout(crate::music::ops::TRACK_INFO_TIMEOUT, playing.get_info()).await
        {
            Ok(Ok(info)) => info.position,
            _ => Duration::ZERO,
        };
    let between = match mode {
        // The new track is next: nothing is between them.
        Mode::Next => &[][..],
        // The new track is last: everything but the two ends is between.
        _ => queue.get(1..queue.len() - 1).unwrap_or_default(),
    };
    let mut lengths = Vec::with_capacity(between.len());
    for track in between {
        lengths.push(length_of(track).await);
    }
    time_until_play(length_of(playing).await, elapsed, &lengths)
}

async fn length_of(track: &TrackHandle) -> Option<Duration> {
    get_track_handle_metadata(track)
        .await
        .ok()
        .and_then(|m| m.duration)
}

/// The wait before a new track: the rest of the one playing (`elapsed` into
/// `playing`), plus every length in `between`. `None` if any length is
/// unknown or rounds to zero, the same rule as `format::duration_text`.
fn time_until_play(
    playing: Option<Duration>,
    elapsed: Duration,
    between: &[Option<Duration>],
) -> Option<Duration> {
    let known = |d: Option<Duration>| d.filter(|d| d.as_secs() > 0);
    let mut wait = known(playing)?.saturating_sub(elapsed);
    for length in between {
        wait = wait.checked_add(known(*length)?)?;
    }
    Some(wait)
}

use crate::sources::rusty_ytdl::RequestOptionsBuilder;
use rusty_ytdl::search::YouTube;
/// Add tracks to the queue from aux_metadata.
#[cfg(not(tarpaulin_include))]
pub async fn queue_aux_metadata(
    ctx: Context<'_>,
    aux_metadata: &[NewAuxMetadata],
    msg: Message,
) -> CrackedResult<()> {
    // use crate::http_utils;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let search_results = aux_metadata;

    let client = &ctx.data().http_client;
    let manager = ctx.data().songbird.clone();

    let call = crate::commands::connected_call(&manager, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;

    let req = RequestOptionsBuilder::new()
        .set_client(client.clone())
        .build();
    let rusty_ytdl = YouTube::new_with_options(&req)?;
    let transport = DiscordTransport::of(ctx.serenity_context());
    for metadata in search_results {
        let source_url = metadata.metadata().source_url.as_ref();
        let metadata_final = if source_url.is_none() || source_url.unwrap().is_empty() {
            let search_query = build_query_aux_metadata(metadata.metadata());
            let _ = courier::edit_rendered_message(
                &transport,
                msg.channel_id,
                msg.id,
                Rendered::text(queuing_text(&search_query)),
            )
            .await;

            let res = rusty_ytdl.search_one(search_query, None).await?;
            let res = res.ok_or(CrackedError::Other("No results found"))?;
            let new_aux_metadata = search_result_to_aux_metadata(&res);

            NewAuxMetadata(new_aux_metadata)
        } else {
            metadata.clone()
        };

        let query_type = QueryType::VideoLink(
            metadata_final
                .metadata()
                .source_url
                .as_ref()
                .cloned()
                .expect("source_url does not exist"),
        );
        let _ = queue_track_back(ctx, &call, &query_type).await?;
    }

    let queue = call.lock().await.queue().current_queue();
    update_queue_messages(
        &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
        ctx.data(),
        &queue,
        guild_id,
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod degraded_perms_notice_tests {
    use super::*;
    use crate::music::perms::{TextKind, TextPerms, TEXT_REQUIRED};
    use poise::serenity_prelude::all::{GenericChannelId, Permissions};

    fn perms(granted: Permissions) -> TextPerms {
        TextPerms {
            channel: GenericChannelId::new(1),
            granted,
            kind: TextKind::Channel,
        }
    }

    #[test]
    fn whole_text_perms_add_no_notice() {
        let note = degraded_notice(Some(&perms(TEXT_REQUIRED)));
        assert!(
            note.is_none(),
            "a guild with every permission must see nothing: {note:?}"
        );
    }

    #[test]
    fn absent_perms_add_no_notice() {
        // `resolve` returned None (cache miss). Fail open: say nothing.
        assert!(degraded_notice(None).is_none());
    }

    #[test]
    fn a_missing_embed_links_is_named_in_the_notice() {
        let note = degraded_notice(Some(&perms(TEXT_REQUIRED - Permissions::EMBED_LINKS)))
            .expect("degraded perms must produce a notice");
        assert!(note.contains("Embed Links"), "got {note}");
        assert!(
            note.contains("/diagnose"),
            "the notice must point at the diagnostic: {note}"
        );
    }

    #[test]
    fn several_missing_permissions_are_all_named() {
        let note = degraded_notice(Some(&perms(Permissions::VIEW_CHANNEL)))
            .expect("degraded perms must produce a notice");
        assert!(note.contains("Send Messages"), "got {note}");
        assert!(note.contains("Embed Links"), "got {note}");
    }

    #[test]
    fn a_missing_embed_links_is_delivered_as_content_not_as_an_embed_field() {
        // 🪤 The one case where an embed field is invisible. On a prefix
        // `r!play` Discord strips the embed from a message in a channel
        // without EMBED_LINKS, so a note saying "Missing **Embed Links**
        // here" attached to that embed reaches nobody -- the reader who needs
        // it is the only one who never sees it.
        match degraded_delivery(Some(&perms(TEXT_REQUIRED - Permissions::EMBED_LINKS))) {
            Some(NoticeDelivery::Content(note)) => {
                assert!(note.contains("Embed Links"), "got {note}");
            },
            other => panic!("a note about EMBED_LINKS must survive the embed: {other:?}"),
        }
    }

    #[test]
    fn a_degradation_that_still_renders_embeds_rides_in_the_embed() {
        match degraded_delivery(Some(&perms(TEXT_REQUIRED - Permissions::SEND_MESSAGES))) {
            Some(NoticeDelivery::Field(note)) => {
                assert!(note.contains("Send Messages"), "got {note}");
            },
            other => panic!("embeds render here, so the note belongs in one: {other:?}"),
        }
    }

    #[test]
    fn whole_perms_are_delivered_nowhere() {
        assert!(degraded_delivery(Some(&perms(TEXT_REQUIRED))).is_none());
        assert!(degraded_delivery(None).is_none());
    }
}

/// 🪤 The tests above all exercise the private helpers in isolation. Delete
/// the notice from `build_play_reply` and the `text_perms` resolve at the call
/// site, and every one of them still passes while the feature does nothing at
/// all -- which is the same defect as a guard test that asserts a hard-coded
/// count. These pin the **wiring**: what the built reply actually carries.
#[cfg(test)]
mod degraded_notice_wiring_tests {
    use super::*;
    use crate::music::perms::{TextKind, TextPerms, TEXT_REQUIRED};
    use poise::serenity_prelude::all::{GenericChannelId, Permissions};

    fn perms(granted: Permissions) -> TextPerms {
        TextPerms {
            channel: GenericChannelId::new(1),
            granted,
            kind: TextKind::Channel,
        }
    }

    /// Build the reply the play path would send. An empty queue takes the
    /// `Ordering::Less` branch, which needs no `TrackHandle` and therefore no
    /// Discord, no songbird and no network.
    async fn reply(text: Option<&TextPerms>) -> (String, Option<String>) {
        let out = build_play_reply(
            &[],
            Mode::End,
            NewQueryType(QueryType::Keywords("anything".to_string())),
            text,
        )
        .await;
        // `CreateEmbed` is a third-party builder with no accessors, so its
        // own `Serialize` is the only way to read one back. An opaque
        // payload whose shape we do not own is exactly what `Value` is for.
        let rendered =
            serde_json::to_value(out.embed.as_ref().expect("the play reply is an embed"))
                .expect("CreateEmbed serialises")
                .to_string();
        (rendered, out.content)
    }

    /// The spec's missing-`EMBED_LINKS` case, as Discord receives it: a
    /// prefix `r!play` edits its placeholder into the reply (poise's
    /// `to_prefix_edit`), and in a channel without `EMBED_LINKS` that edit's
    /// content is the only part anyone sees. The note is there; the embed
    /// is kept for whoever can see it.
    #[tokio::test]
    async fn without_embed_links_the_notice_is_in_the_content_and_the_embed_is_kept() {
        let out = build_play_reply(
            &[],
            Mode::End,
            NewQueryType(QueryType::Keywords("anything".to_string())),
            Some(&perms(TEXT_REQUIRED - Permissions::EMBED_LINKS)),
        )
        .await;
        let edit = out
            .to_reply_edit()
            .0
            .to_prefix_edit(serenity::EditMessage::new());
        let v = serde_json::to_value(edit).expect("EditMessage serialises");
        assert_eq!(
            v["content"],
            "⚠️ Missing **Embed Links** here — I won't post now-playing. `/diagnose` for detail."
        );
        let embeds = v["embeds"].as_array().expect("the edit carries embeds");
        assert_eq!(embeds.len(), 1, "{v}");
        assert_eq!(embeds[0]["description"], "No tracks in queue!");
    }

    #[tokio::test]
    async fn a_degraded_text_channel_actually_reaches_the_built_embed() {
        let (embed, content) =
            reply(Some(&perms(TEXT_REQUIRED - Permissions::SEND_MESSAGES))).await;
        assert!(
            embed.contains("Limited permissions"),
            "the notice never reached the embed: {embed}"
        );
        assert!(embed.contains("Send Messages"), "got {embed}");
        assert!(
            content.is_none(),
            "embeds render here, so nothing needs to escape one: {content:?}"
        );
    }

    #[tokio::test]
    async fn whole_text_perms_leave_the_built_reply_unmarked() {
        let (embed, content) = reply(Some(&perms(TEXT_REQUIRED))).await;
        assert!(
            !embed.contains("Limited permissions"),
            "nothing is wrong, so the embed must say nothing: {embed}"
        );
        assert!(content.is_none(), "got {content:?}");
    }

    #[tokio::test]
    async fn absent_perms_leave_the_built_reply_unmarked() {
        // `resolve` returned None (cache miss). Fail open: say nothing.
        let (embed, content) = reply(None).await;
        assert!(!embed.contains("Limited permissions"), "got {embed}");
        assert!(content.is_none(), "got {content:?}");
    }

    #[tokio::test]
    async fn a_missing_embed_links_leaves_the_embed_and_rides_in_the_content() {
        // 🪤 The whole point of I3. This note must NOT be in the embed --
        // Discord strips the embed in exactly this channel -- and it must be
        // somewhere the caller can still deliver it.
        let (embed, content) = reply(Some(&perms(TEXT_REQUIRED - Permissions::EMBED_LINKS))).await;
        assert!(
            !embed.contains("Limited permissions"),
            "an embed that will be stripped must not be the only carrier: {embed}"
        );
        let content = content.expect("the note must survive outside the embed");
        assert!(content.contains("Embed Links"), "got {content}");
        assert!(
            content.contains("/diagnose"),
            "the notice must point at the diagnostic: {content}"
        );
    }
}

#[cfg(test)]
mod time_until_play_tests {
    use super::*;
    use crate::music::audit::{Actor, BotReason};
    use crate::music::ops::test_support::{offline_call, GUILD};
    use crate::music::queue::enqueue_input_back;
    use crate::music::PlaybackOwner;
    use crate::{Data, DataInner};
    use songbird::input::AuxMetadata;

    /// An offline queue whose tracks have these lengths, in seconds. The call
    /// is returned so it outlives the handles.
    async fn queue_of(lengths: &[Option<u64>]) -> (Arc<Mutex<Call>>, Vec<TrackHandle>) {
        let data = Data(Arc::new(DataInner::default()));
        let call = offline_call();
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, Actor::bot(BotReason::Autopause))
            .await
            .unwrap();
        for (i, length) in lengths.iter().enumerate() {
            let metadata = AuxMetadata {
                title: Some(format!("t{i}")),
                duration: length.map(Duration::from_secs),
                ..Default::default()
            };
            let source = songbird::input::File::new(format!("/nonexistent/{i}.opus")).into();
            enqueue_input_back(&guard, &call, source, Some(metadata), None).await;
        }
        let queue = call.lock().await.queue().current_queue();
        (call, queue)
    }

    /// The wait for the last track of `lengths`, bounded: `get_info` never
    /// answers on an offline call, and the reply must not wait for it.
    async fn wait_for(lengths: &[Option<u64>], mode: Mode) -> Option<Duration> {
        let (_call, queue) = queue_of(lengths).await;
        tokio::time::timeout(
            Duration::from_secs(5),
            calculate_time_until_play(&queue, mode),
        )
        .await
        .expect("calculate_time_until_play outlived its bound")
    }

    /// Each track ahead counts its own length. It used to count the playing
    /// track's length for every one of them, so a live stream in the middle
    /// went unnoticed. (Offline, the playing track reads as just started.)
    #[tokio::test]
    async fn the_wait_adds_up_each_track_ahead() {
        assert_eq!(
            wait_for(&[Some(100), Some(30), Some(10)], Mode::End).await,
            Some(Duration::from_secs(130))
        );
        assert_eq!(
            wait_for(&[Some(100), None, Some(10)], Mode::End).await,
            None
        );
        // Queued next: only the playing track is ahead of it.
        assert_eq!(
            wait_for(&[Some(100), None, Some(10)], Mode::Next).await,
            Some(Duration::from_secs(100))
        );
    }

    /// The queued reply, end to end: the card is the new track's, with its
    /// length and the wait ahead of it; behind a live stream, the length only.
    #[tokio::test]
    async fn a_queued_reply_says_the_new_tracks_length_and_its_wait() {
        let footer = |lengths: &'static [Option<u64>]| async move {
            let (_call, queue) = queue_of(lengths).await;
            let out = build_play_reply(
                &queue,
                Mode::End,
                NewQueryType(QueryType::Keywords("anything".into())),
                None,
            )
            .await;
            let v = serde_json::to_value(out.embed.expect("the play reply is an embed")).unwrap();
            assert_eq!(v["author"]["name"], PLAY_QUEUE);
            assert_eq!(v["title"], "t1");
            v["footer"]["text"].as_str().map(str::to_owned)
        };
        assert_eq!(
            footer(&[Some(100), Some(273)]).await.as_deref(),
            Some("Track duration: 4:33\nEstimated time until play: 1:40")
        );
        assert_eq!(
            footer(&[None, Some(273)]).await.as_deref(),
            Some("Track duration: 4:33")
        );
    }

    /// "Estimated time until play: ∞" (a live stream playing) and "00:00" (a
    /// length nobody knew) were both computed from an unknown length.
    #[test]
    fn an_unknown_length_ahead_means_no_estimate() {
        let s = |secs| Some(Duration::from_secs(secs));
        assert_eq!(time_until_play(None, Duration::ZERO, &[]), None);
        assert_eq!(time_until_play(s(100), Duration::ZERO, &[None]), None);
        assert_eq!(time_until_play(s(100), Duration::ZERO, &[s(0)]), None);
        assert_eq!(time_until_play(s(0), Duration::ZERO, &[]), None);
    }

    #[test]
    fn the_estimate_is_what_is_left_of_the_playing_track_plus_the_rest() {
        let s = |secs| Some(Duration::from_secs(secs));
        assert_eq!(time_until_play(s(100), Duration::from_secs(40), &[]), s(60));
        assert_eq!(
            time_until_play(s(100), Duration::from_secs(40), &[s(30), s(5)]),
            s(95)
        );
        // A position past the reported end (it happens) is not a panic.
        assert_eq!(
            time_until_play(s(100), Duration::from_secs(130), &[s(30)]),
            s(30)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::messages::PLAY_PLAYLIST;
    use crate::messaging::render::description;

    #[test]
    fn the_queuing_line_escapes_the_query() {
        assert_eq!(
            queuing_text("[x](https://evil.example) @everyone"),
            r"Queuing... \[x\](https://evil.example) \@everyone"
        );
        assert_eq!(queuing_text("plain"), "Queuing... plain");
    }

    #[test]
    fn a_playlist_line_escapes_the_title_and_keeps_the_wording() {
        assert_eq!(
            playlist_line(
                "[a](b) *x*",
                "https://youtu.be/x",
                Some(Duration::from_secs(201))
            ),
            r"[\[a\](b) \*x\*](https://youtu.be/x) • `3:21`"
        );
    }

    #[test]
    fn a_playlist_line_with_no_title_or_no_http_link_is_still_sane() {
        let ten = Some(Duration::from_secs(10));
        assert_eq!(
            playlist_line("  ", "javascript:alert(1)", ten),
            "(untitled) • `0:10`"
        );
        assert_eq!(
            playlist_line("t", "https://x.example/a(b)", ten),
            "[t](https://x.example/a%28b%29) • `0:10`"
        );
    }

    /// The length reads `m:ss` like every other music message (it read
    /// `03:21`), and an unknown one is left out, not `00:00` or "Unknown
    /// duration".
    #[test]
    fn a_playlist_line_of_unknown_length_has_no_length() {
        for unknown in [None, Some(Duration::ZERO)] {
            assert_eq!(
                playlist_line("t", "https://x.example/1", unknown),
                "[t](https://x.example/1)",
                "{unknown:?}"
            );
        }
        assert_eq!(
            playlist_line("t", "https://x.example/1", Some(Duration::from_secs(3600))),
            "[t](https://x.example/1) • `1:00:00`"
        );
    }

    #[test]
    fn a_playlist_display_is_one_line_per_track() {
        let out = playlist_display(
            [
                ("a", "https://x.example/1", 60),
                ("b", "https://x.example/2", 120),
            ]
            .into_iter()
            .map(|(a, b, c)| (a.to_owned(), b.to_owned(), Some(Duration::from_secs(c)))),
        );
        assert_eq!(
            out,
            "[a](https://x.example/1) • `1:00`\n[b](https://x.example/2) • `2:00`"
        );
    }

    /// 🪤 Seen on production v0.12.1: a playlist `/play` replied with the literal
    /// text "PlaylistQueued", `{:?}` of the message instead of its text.
    #[tokio::test]
    async fn a_queued_playlist_is_announced_in_words() {
        let (_data, call, _ids, _rx) = crate::music::ops::test_support::queue_of(2).await;
        let queue = call.lock().await.queue().current_queue();
        let out = build_play_reply(
            &queue,
            Mode::End,
            NewQueryType(QueryType::PlaylistLink(
                "https://www.youtube.com/playlist?list=x".into(),
            )),
            None,
        )
        .await;
        let text = description(&out).expect("the play reply is an embed");

        assert_eq!(text, PLAY_PLAYLIST);
        assert!(!text.contains("PlaylistQueued"), "{text}");
    }
}
