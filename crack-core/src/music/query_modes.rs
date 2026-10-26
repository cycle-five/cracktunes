//! The poise half of the query type: the `mode_*` methods that queue a query
//! for the command that asked, and `query_type_from_url`, which reads the
//! guild's settings through the command's context.

use super::{
    query::{
        play_all_plan, spotify_query, unresolved_spotify_link, NewQueryType, PlayAll, Queries,
        ResolvedQuery,
    },
    queue::{
        queue_keyword_list_back, queue_query_list_offset, queue_resolved_list_back,
        queue_track_back, queue_track_front,
    },
};
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::interface::create_search_response;
use crate::{
    errors::CrackedError,
    http_utils,
    http_utils::check_banned_domains,
    messaging::interface::{send_no_query_provided, send_search_failed},
    utils::yt_search_select,
    Context, CrackedResult, Error,
};
use ::serenity::all::{Attachment, CreateAttachment, CreateMessage};
use colored::Colorize;
use crack_types::{ytdl_for_url, QueryType, SpotifyTrackTrait as _};
use poise::ReplyHandle;
use rusty_ytdl::search::Playlist;
use songbird::{
    input::{Compose as _, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use url::Url;

/// The ways `/play` queues a query, each tied to the invoking command's
/// context. An extension trait rather than inherent methods, so
/// [`NewQueryType`] can move to another crate without poise following it.
pub trait QueryModes {
    fn mode_download(
        &self,
        ctx: Context<'_>,
        mp3: bool,
    ) -> impl std::future::Future<Output = Result<bool, CrackedError>>;

    fn mode_search(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
    ) -> impl std::future::Future<Output = Result<Vec<TrackHandle>, CrackedError>>;

    fn mode_search_keywords(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        keywords: String,
    ) -> impl std::future::Future<Output = Result<Vec<TrackHandle>, CrackedError>>;

    fn mode_next(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'_>,
    ) -> impl std::future::Future<Output = Result<bool, CrackedError>>;

    fn mode_end2<'ctx>(
        &self,
        ctx: Context<'ctx>,
        call: Arc<Mutex<Call>>,
        _search_reply: ReplyHandle<'ctx>,
    ) -> impl std::future::Future<Output = CrackedResult<bool>>;

    fn mode_end<'ctx>(
        &self,
        ctx: Context<'ctx>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'ctx>,
    ) -> impl std::future::Future<Output = Result<bool, CrackedError>>;

    fn mode_rest(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'_>,
    ) -> impl std::future::Future<Output = Result<bool, CrackedError>>;

    fn mode_jump(
        &self,
        _ctx: Context<'_>,
        _call: Arc<Mutex<Call>>,
    ) -> impl std::future::Future<Output = Result<bool, CrackedError>>;
}

impl QueryModes for NewQueryType {
    async fn mode_download(&self, ctx: Context<'_>, mp3: bool) -> Result<bool, CrackedError> {
        let (status, file_name) = self.get_download_status_and_filename(mp3).await?;
        // Not through the courier: the file is the message, and `Rendered`
        // carries no attachments.
        #[expect(
            clippy::disallowed_methods,
            reason = "carries a file attachment; Rendered has none"
        )]
        ctx.channel_id()
            .send_message(
                ctx.http(),
                CreateMessage::new()
                    .content(format!("Download status {}", status))
                    .add_file(CreateAttachment::path(Path::new(&file_name))?),
            )
            .await?;

        Ok(false)
    }

    async fn mode_search(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        let NewQueryType(qt) = self;
        match qt.clone() {
            QueryType::Keywords(keywords) => {
                self.mode_search_keywords(ctx, call, keywords.clone()).await
            },
            QueryType::SpotifyTracks(tracks) => {
                self.mode_search_keywords(
                    ctx,
                    call,
                    tracks
                        .iter()
                        .map(|x| x.build_query())
                        .collect::<Vec<String>>()
                        .join(" "),
                )
                .await
            },
            QueryType::YoutubeSearch(query) => {
                self.mode_search_keywords(ctx, call, query.clone()).await
            },
            _ => send_search_failed(&ctx).await.map(|_| vec![]),
        }
    }

    async fn mode_search_keywords(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        keywords: String,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        //let reqwest_client = ctx.data().http_client.clone();
        let search_results = YoutubeDl::new_search(http_utils::get_client_old().clone(), keywords)
            .search(None)
            .await?
            .collect::<Vec<_>>();

        // let user_id = ctx.author().id;
        let qt = yt_search_select(
            ctx.serenity_context().clone(),
            ctx.channel_id(),
            search_results,
        )
        .await?;
        queue_track_back(ctx, &call, &qt).await
        // update_queue_messages(ctx, ctx.data(), &queue, guild_id).await
    }

    async fn mode_next(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'_>,
    ) -> Result<bool, CrackedError> {
        let search_msg = &mut search_reply.into_message().await?;
        let NewQueryType(qt) = self;
        match qt {
            QueryType::SpotifyLink(url) => return Err(unresolved_spotify_link(url)),
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
                tracing::info!("Mode::Next, QueryType::Keywords|VideoLink|File|NewYoutubeDl");
                queue_track_front(ctx, &call, qt).await?;
            },
            QueryType::PlaylistLink(url) => {
                let playlist: Playlist = rusty_ytdl::search::Playlist::get(
                    url.clone(),
                    Some(&rusty_ytdl::search::PlaylistSearchOptions {
                        limit: ctx.data().bot_settings.get_playlist_limit(),
                        ..Default::default()
                    }),
                )
                .await?;
                // queue_query_list_offset acquires its own guard internally,
                // after it resolves -- no guard passed in here. See its doc
                // comment.
                queue_query_list_offset(ctx, call, Queries::from(playlist).to_vec(), 1, search_msg)
                    .await?;
            },
            QueryType::KeywordList(keywords_list) => {
                queue_query_list_offset(
                    ctx,
                    call,
                    Queries::from(keywords_list.clone()).to_vec(),
                    1,
                    search_msg,
                )
                .await?;
            },
            QueryType::SpotifyTracks(tracks) => {
                // let keywords_list = tracks
                //     .iter()
                //     .map(|x| x.build_query())
                //     .collect::<Vec<String>>();
                queue_query_list_offset(
                    ctx,
                    call,
                    Queries::from(tracks.clone()).to_vec(),
                    1,
                    search_msg,
                )
                .await?;
            },
            QueryType::YoutubeSearch(_) => {
                return Err(CrackedError::Other("Not implemented yet!"));
            },
            QueryType::None => {
                return Ok(false);
            },
        }
        Ok(true)
    }

    async fn mode_end2<'ctx>(
        &self,
        ctx: Context<'ctx>,
        call: Arc<Mutex<Call>>,
        _search_reply: ReplyHandle<'ctx>,
    ) -> CrackedResult<bool> {
        let NewQueryType(qt) = self;
        match queue_track_back(ctx, &call, qt).await {
            Ok(_) => (),
            Err(e) => {
                tracing::error!("queue_track_back error: {:?}", e);
                return Err(e);
            },
        };
        Ok(true)
    }

    async fn mode_end<'ctx>(
        &self,
        ctx: Context<'ctx>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'ctx>,
    ) -> Result<bool, CrackedError> {
        let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
        let search_msg = &search_reply.clone().into_message().await?;
        let NewQueryType(qt) = self;
        match qt {
            QueryType::SpotifyLink(url) => Err(unresolved_spotify_link(url)),
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::End, QueryType::YoutubeSearch");

                let res =
                    YoutubeDl::new_search(http_utils::get_client_old().clone(), query.clone())
                        .search(None)
                        .await?
                        .collect();
                let user_id = ctx.author().id;
                create_search_response(&ctx, guild_id, user_id, query.clone(), res).await?;
                Ok(true)
            },
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::NewYoutubeDl(_) => {
                let NewQueryType(qt) = self;
                tracing::warn!("### Mode::End, QueryType::Keywords | QueryType::VideoLink");
                match queue_track_back(ctx, &call, qt).await {
                    Ok(_) => (),
                    Err(e) => {
                        tracing::error!("queue_track_back error: {:?}", e);
                        return Err(e);
                    },
                };
                Ok(true)
            },
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::End, QueryType::PlaylistLink");
                // A playlist fetch already returns title / duration / url for
                // every entry, so the tracks go straight into the queue. The
                // old path threw that away and re-resolved each entry with a
                // separate (serial) yt-dlp invocation.
                let tracks = ctx
                    .data()
                    .ct_client
                    .resolve_playlist_limit(url, ctx.data().bot_settings.get_playlist_limit())
                    .await?;
                queue_resolved_list_back(ctx, call, tracks, search_msg).await?;
                Ok(true)
            },
            QueryType::SpotifyTracks(tracks) => {
                let queries = tracks
                    .iter()
                    .map(|x| QueryType::Keywords(x.build_query()))
                    .collect::<Vec<QueryType>>();
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::End, QueryType::KeywordList");
                let queries = keywords_list
                    .iter()
                    .map(|x| QueryType::Keywords(x.clone()))
                    .collect::<Vec<QueryType>>();
                // Was a serial `queue_track_back` per keyword; the batched
                // path resolves them concurrently.
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            QueryType::File(file) => {
                tracing::trace!("Mode::End, QueryType::File");
                let _queue = queue_track_back(ctx, &call, &QueryType::File(file.clone())).await?;
                // update_queue_messages(ctx.http(), ctx.data(), &queue, guild_id).await;
                Ok(true)
            },
            QueryType::None => send_no_query_provided(&ctx).await.map(|_| false),
        }
    }

    async fn mode_rest(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'_>,
    ) -> Result<bool, CrackedError> {
        let NewQueryType(qt) = self;
        match play_all_plan(qt)? {
            PlayAll::Link(url) => {
                // FIXME
                let mut src = ytdl_for_url(http_utils::get_client_old().clone(), url)?;
                let metadata = src.aux_metadata().await?;
                queue_track_back(ctx, &call, &QueryType::NewYoutubeDl((src, metadata))).await?;
                Ok(true)
            },
            PlayAll::Searches(queries) => {
                let search_msg = search_reply.into_message().await?;
                queue_keyword_list_back(ctx, call, queries, &search_msg).await?;
                Ok(true)
            },
        }
    }

    async fn mode_jump(
        &self,
        _ctx: Context<'_>,
        _call: Arc<Mutex<Call>>,
    ) -> Result<bool, CrackedError> {
        Err(CrackedError::Other("Not implemented yet!"))
    }
}

/// Returns the QueryType for a given URL (or query string, or file attachment)
///
/// This carried an `#[allow(dead_code)]` from 2024-11-24 (`4195522`), added
/// while `commands::music` was commented out of the build for the module port.
/// The module came back two days later in `3098fd8` and the attribute did not
/// go with it; both callers -- `doplay` and `get_metadata` -- have been live
/// ever since.
pub async fn query_type_from_url(
    ctx: Context<'_>,
    url: &str,
    file: Option<Attachment>,
) -> Result<Option<ResolvedQuery>, Error> {
    tracing::info!("url: {}", url);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    // Set by the Spotify branches below; every other query kind has no listing
    // and so nothing to be short of.
    let mut shortfall = None;
    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") | Some("play.spotify.com") | Some("spotify.link") => {
                // Spotify is resolved to *searches*, never to audio: sleevenote
                // gives us titles and artists, and the tracks themselves are
                // found on YouTube exactly as the old rspotify path did. A
                // collection becomes a `KeywordList`, which the queueing path
                // already resolves concurrently.
                let (query, short) = spotify_query(url).await?;
                shortfall = short;
                Some(query)
            },
            Some("cdn.discordapp.com") => {
                tracing::info!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            },
            Some("www.youtube.com") => {
                let has_list = url_data
                    .query_pairs()
                    .any(|(key, _)| key == "list" || key == "playlist");
                let has_video = url_data.query_pairs().any(|(key, _)| key == "v");
                // 🔒 A playlist link is carried on as a URL rebuilt from its
                // validated id, never as the raw `list=` value. That value used
                // to be passed along as-is and reach yt-dlp as its first
                // positional argument, where `list=--batch-file%3D…` made it an
                // option. See `canonical_youtube_playlist_url`.
                match crack_types::canonical_youtube_playlist_url(url) {
                    Some(playlist) => {
                        tracing::warn!(
                            "{}: {}",
                            "youtube playlist".blue(),
                            playlist.underline().blue()
                        );
                        Some(QueryType::PlaylistLink(playlist))
                    },
                    // Nothing but a bad `list=` to go on.
                    None if has_list && !has_video => {
                        return Err(CrackedError::InvalidPlaylist.into());
                    },
                    // A video link, perhaps with a `list=` we don't accept.
                    // The whole URL goes on, and it starts with `https://`, so
                    // yt-dlp cannot take it for an option.
                    None => {
                        tracing::warn!("{}: {}", "youtube video".blue(), url.underline().blue());
                        Some(QueryType::VideoLink(url.to_string()))
                    },
                }
            },
            // For all other domains fall back to yt-dlp.
            Some(other) => {
                // 🔒 yt-dlp's generic extractor fetches whatever it is handed,
                // so a link to 127.0.0.1, the LAN or a metadata endpoint is
                // refused before it gets there.
                if !crack_types::is_public_http_url(&url_data).await {
                    tracing::warn!("refusing non-public link: {}", url);
                    return Err(CrackedError::UrlNotAllowed.into());
                }
                tracing::warn!("query_type_from_url: domain: {other}, using yt-dlp");
                tracing::warn!(
                    "query_type_from_use: {}: {}",
                    "LINK".blue(),
                    url.underline().blue()
                );
                let mut ytdl = ytdl_for_url(http_utils::get_client_old().clone(), url)?;
                // This can fail whenever yt-dlp cannot parse a track from the URL.
                let metadata = match ytdl.aux_metadata().await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::error!("yt-dlp error: {}", e);
                        return Err(CrackedError::AudioStream(e).into());
                    },
                };
                Some(QueryType::NewYoutubeDl((ytdl, metadata)))
            },
            None => {
                // handle spotify:track:3Vr5jdQHibI2q0A0KW4RWk format?
                // TODO: Why is this a thing?
                if url.starts_with("spotify:") {
                    // `spotify:album:<id>` is as valid as `spotify:track:<id>`,
                    // so the kind is read from the URI rather than assumed to
                    // be a track, which is what the old path did -- it built a
                    // /track/ URL out of an album id and looked up nothing.
                    let (query, short) = spotify_query(url).await?;
                    shortfall = short;
                    Some(query)
                } else {
                    Some(QueryType::Keywords(url.to_string()))
                }
            },
        },
        Err(e) => {
            // 🪤 This is the ORDINARY path, not a failure. `Url::parse` returns
            // `RelativeUrlWithoutBase` for anything that is not a URL -- which
            // is to say, for every `/play bohemian rhapsody`. Search keywords
            // are what most people give this command, and the next line handles
            // them correctly; logging ERROR first said the opposite.
            //
            // Observed in production filling the log alongside #511 and #512.
            // Kept at `debug!` rather than deleted because a *different* parse
            // failure here would still be worth seeing.
            tracing::debug!("not a URL, treating as search keywords: {}", e);
            Some(QueryType::Keywords(url.to_string()))
        },
    };
    let guild_settings = ctx
        .data()
        .get_guild_settings(guild_id)
        .await
        .ok_or(CrackedError::NoGuildSettings)?;
    let query = check_banned_domains(&guild_settings, query_type.map(NewQueryType))?;
    Ok(query.map(|query| ResolvedQuery { query, shortfall }))
}
