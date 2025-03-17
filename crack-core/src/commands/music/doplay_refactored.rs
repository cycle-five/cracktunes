use crate::{
    commands::{cmd_check_music, get_call_or_join_author, help, resume::resume_internal},
    handlers::track_end::update_queue_messages,
    http_utils::SendMessageParams,
    messaging::interface::{create_now_playing_embed, send_search_message},
    messaging::message::CrackedMessage,
    music::{
        queue_manager::{QueueManager, QueuePosition},
        resolver::TrackResolver,
        track::Track,
    },
    poise_ext::{ContextExt, PoiseContextExt},
    Context, Error,
};
use ::serenity::all::CreateAutocompleteResponse;
use crack_testing::suggestion2;
use crack_types::CrackedError;
use crack_types::{
    get_human_readable_timestamp,
    messaging::messages::{
        PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, TRACK_DURATION, TRACK_TIME_TO_PLAY,
    },
};
use poise::{serenity_prelude as serenity, CreateReply};
use songbird::tracks::TrackHandle;
use std::{borrow::Cow, sync::Arc, time::Duration};

/// Get the guild name.
/// # Errors
/// This function can error if the guild name can't be found.
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    let shard_id = ctx.serenity_context().shard_id;
    ctx.say(format!(
        "The name of this guild is: {}, shard_id: {}",
        ctx.partial_guild().await.unwrap().name,
        shard_id
    ))
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
    #[description = "Song link or search query."]
    #[autocomplete = "autocomplete"]
    query: String,
) -> Result<(), Error> {
    let query = query.split('~').next().unwrap_or_default().to_string();
    play_internal(ctx, Some("next".to_string()), None, Some(query)).await
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
    #[description = "Search query."]
    query: String,
) -> Result<(), Error> {
    play_internal(ctx, Some("search".to_string()), None, Some(query)).await
}

/// Autocomplete to suggest a search query.
pub async fn autocomplete<'a>(
    _ctx: poise::ApplicationContext<'_, crate::Data, Error>,
    searching: &'a str,
) -> CreateAutocompleteResponse<'a> {
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
    let query = query.split('~').next().unwrap_or_default().to_string();
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

    // Join voice channel
    let call = get_call_or_join_author(ctx).await?;

    // Create resolver and queue manager
    let resolver = TrackResolver::new(ctx.get_http_client());
    let queue_manager = QueueManager::new(call.clone());

    // Send initial message
    let _search_msg = send_search_message(&ctx).await?;

    // Resolve playlist
    let track_collection = resolver.resolve_playlist(&query).await?;

    // Display playlist info
    let playlist_info = format!("Queuing playlist with {} tracks", track_collection.len());
    ctx.send_reply_embed(CrackedMessage::Other(playlist_info))
        .await?;

    // Add tracks to queue
    queue_manager
        .add_tracks(track_collection, QueuePosition::End, &resolver)
        .await?;

    // Get updated queue
    let queue = queue_manager.get_queue().await;

    // Update queue messages
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;

    // Send confirmation
    ctx.send_reply_embed(CrackedMessage::PlaylistQueued).await?;

    Ok(())
}

/// Parse mode string to QueuePosition
pub(crate) fn parse_mode(mode_str: Option<String>, is_prefix: bool) -> QueuePosition {
    if is_prefix {
        // Parse from prefix command
        match mode_str.as_deref() {
            Some("next") => QueuePosition::Next,
            Some("front") => QueuePosition::Front,
            Some("search") => QueuePosition::End, // Search is handled separately
            Some("jump") => QueuePosition::End,   // Jump is handled separately
            Some("downloadmkv") | Some("downloadmp3") => QueuePosition::End, // Download is handled separately
            _ => QueuePosition::End,
        }
    } else {
        // Parse from slash command
        match mode_str.as_deref() {
            Some("next") => QueuePosition::Next,
            Some("front") => QueuePosition::Front,
            _ => QueuePosition::End,
        }
    }
}

/// Handle empty query case
async fn handle_empty_query(ctx: Context<'_>) -> Result<(), Error> {
    // If paused, resume playback
    if ctx.is_paused().await.unwrap_or_default() {
        return resume_internal(ctx).await;
    }

    // Otherwise, send error message
    let msg_params = SendMessageParams::default()
        .with_channel(ctx.channel_id())
        .with_msg(CrackedMessage::CrackedError(CrackedError::NoQuery))
        .with_color(crate::serenity::Color::RED);

    ctx.send_message(msg_params).await?;
    Ok(())
}

/// Resolve query to track
async fn resolve_query(
    ctx: Context<'_>,
    query_or_url: Option<String>,
    file: Option<serenity::Attachment>,
    resolver: &TrackResolver,
) -> Result<Track, Error> {
    match (query_or_url, file) {
        (Some(query), None) => {
            // Parse query to track
            let mut track = resolver.parse_query(&query).await?;
            // Set user ID
            track.user_id = Some(ctx.author().id);
            Ok(track)
        },
        (None, Some(file)) => {
            // Parse file to track
            let mut track = resolver.parse_file(file).await?;
            // Set user ID
            track.user_id = Some(ctx.author().id);
            Ok(track)
        },
        (Some(_), Some(_)) => {
            // Both query and file provided, prioritize file
            Err(CrackedError::Other("Cannot provide both query and file").into())
        },
        (None, None) => {
            // Neither query nor file provided
            Err(CrackedError::NoQuery.into())
        },
    }
}

/// Handle special modes (search, download)
async fn handle_special_mode(
    _ctx: Context<'_>,
    mode_str: Option<String>,
    track: &mut Track,
    _call: Arc<tokio::sync::Mutex<songbird::Call>>,
    resolver: &TrackResolver,
) -> Result<Option<QueuePosition>, Error> {
    match mode_str.as_deref() {
        Some("search") => {
            // Handle search mode
            // This would typically show a search UI and let the user select a track
            // For now, we'll just resolve the track and return it
            resolver.resolve(track).await?;
            Ok(Some(QueuePosition::End))
        },
        Some("downloadmkv") => {
            // Handle download mode (MKV)
            // This would download the track and send it to the user
            // Not implemented in this refactoring
            Err(CrackedError::Other("Download mode not implemented in refactored version").into())
        },
        Some("downloadmp3") => {
            // Handle download mode (MP3)
            // This would download the track and send it to the user
            // Not implemented in this refactoring
            Err(CrackedError::Other("Download mode not implemented in refactored version").into())
        },
        _ => Ok(None),
    }
}

/// Build embed for play response
async fn build_play_embed(
    queue: &[TrackHandle],
    position: QueuePosition,
) -> Result<serenity::builder::CreateEmbed<'static>, Error> {
    use serenity::builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};

    if queue.is_empty() {
        return Ok(CreateEmbed::default()
            .description("No tracks in queue!")
            .footer(CreateEmbedFooter::new("No tracks in queue!")));
    }

    // Get the track to display based on position
    let track = match position {
        QueuePosition::Next => queue.get(1).unwrap_or_else(|| queue.first().unwrap()),
        QueuePosition::Front => queue.first().unwrap(),
        QueuePosition::End => queue.last().unwrap(),
        QueuePosition::At(_) => queue.last().unwrap(),
    };

    // If there's only one track, it's now playing
    if queue.len() == 1 {
        return Ok(create_now_playing_embed(track.clone()).await);
    }

    // Get metadata
    let metadata = crate::utils::get_track_handle_metadata(track)
        .await
        .unwrap_or_default();

    // Get estimated time until play
    let estimated_time = calculate_time_until_play(queue, position.clone())
        .await
        .unwrap_or_default();

    // Build embed
    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();
    let meta_title = metadata.title.clone().unwrap_or(QUEUE_NO_TITLE.to_string());
    let source_url = metadata
        .source_url
        .clone()
        .unwrap_or(QUEUE_NO_SRC.to_string());

    let duration_ts = get_human_readable_timestamp(metadata.duration);
    let estimate_time_ts = get_human_readable_timestamp(Some(estimated_time));
    let footer_text =
        format!("{TRACK_DURATION} {duration_ts}\n{TRACK_TIME_TO_PLAY} {estimate_time_ts}");

    let author_title = match position {
        QueuePosition::Next => PLAY_TOP,
        QueuePosition::Front => PLAY_TOP,
        _ => PLAY_QUEUE,
    };

    let author = CreateEmbedAuthor::new(author_title);

    Ok(CreateEmbed::new()
        .author(author)
        .title(meta_title)
        .url(source_url)
        .thumbnail(thumbnail)
        .footer(CreateEmbedFooter::new(Cow::Owned(footer_text))))
}

/// Calculate the time until the next track plays
async fn calculate_time_until_play(
    queue: &[TrackHandle],
    position: QueuePosition,
) -> Option<Duration> {
    if queue.is_empty() {
        return None;
    }

    let zero_duration = Duration::ZERO;
    let top_track = queue.first()?;
    let top_track_elapsed = top_track
        .get_info()
        .await
        .map(|i| i.position)
        .unwrap_or(zero_duration);
    let metadata = crate::utils::get_track_handle_metadata(top_track)
        .await
        .ok()?;

    let top_track_duration = match metadata.duration {
        Some(duration) => duration,
        None => return Some(Duration::MAX),
    };

    match position {
        QueuePosition::Next | QueuePosition::Front => {
            // For Next or Front, we only need to wait for the current track
            Some(top_track_duration - top_track_elapsed)
        },
        _ => {
            // For End or At, we need to wait for all tracks in between
            let center = &queue[1..queue.len() - 1];
            let livestreams =
                center.len() - center.iter().filter_map(|_t| metadata.duration).count();

            // If any of the tracks before are livestreams, the new track will never play
            if livestreams > 0 {
                return Some(Duration::MAX);
            }

            let durations = center
                .iter()
                .fold(Duration::ZERO, |acc, _x| acc + metadata.duration.unwrap());

            Some(durations + top_track_duration - top_track_elapsed)
        },
    }
}

/// Main play function
pub async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // Start timing
    let start = std::time::Instant::now();

    // Check if we have a query or file
    if query_or_url.is_none() && file.is_none() {
        return handle_empty_query(ctx).await;
    }

    // Parse mode
    let is_prefix = ctx.is_prefix();
    let position = parse_mode(mode.clone(), is_prefix);

    // Join voice channel
    let call = get_call_or_join_author(ctx).await?;

    // Send search message
    let search_msg = send_search_message(&ctx).await?;

    // Create resolver and queue manager
    let resolver = TrackResolver::new(ctx.get_http_client());
    let queue_manager = QueueManager::new(call.clone());

    // Resolve query to track
    let mut track = resolve_query(ctx, query_or_url, file, &resolver).await?;

    // Handle special modes (search, download)
    if let Some(new_position) =
        handle_special_mode(ctx, mode, &mut track, call.clone(), &resolver).await?
    {
        // Use the new position if a special mode was handled
        let _track_handle = queue_manager
            .add_track(track, new_position.clone(), &resolver)
            .await?;

        // Get updated queue
        let queue = queue_manager.get_queue().await;

        // Build and send embed
        let embed = build_play_embed(&queue, new_position).await?;
        search_msg
            .edit(ctx, CreateReply::default().embed(embed))
            .await?;

        return Ok(());
    }

    // Add track to queue
    let _track_handle = queue_manager
        .add_track(track, position.clone(), &resolver)
        .await?;

    // Get updated queue
    let queue = queue_manager.get_queue().await;

    // Build and send embed
    let embed = build_play_embed(&queue, position).await?;
    search_msg
        .edit(ctx, CreateReply::default().embed(embed))
        .await?;

    // Log timing information
    let end = std::time::Instant::now();
    tracing::info!("play_internal took {:?}", end.duration_since(start));

    Ok(())
}
