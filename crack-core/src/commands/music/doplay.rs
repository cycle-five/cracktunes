use super::play_utils::query::QueryType;
use super::play_utils::queue::{get_mode, get_msg, queue_track_back};
use crate::commands::get_call_with_fail_msg;
use crate::commands::play_utils::query::query_type_from_url;
use crate::guild::permissions::command_check_music;
use crate::sources::rusty_ytdl::RustyYoutubeClient;
use ::serenity::all::CommandInteraction;
//FIXME
use crate::utils::edit_embed_response2;
use crate::{
    errors::{verify, CrackedError},
    guild::settings::GuildSettings,
    handlers::track_end::update_queue_messages,
    messaging::interface::create_now_playing_embed,
    messaging::{
        message::CrackedMessage,
        messages::{
            PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, TRACK_DURATION, TRACK_TIME_TO_PLAY,
        },
    },
    sources::spotify::SpotifyTrack,
    sources::youtube::build_query_aux_metadata,
    utils::{get_human_readable_timestamp, get_track_metadata, send_embed_response_poise},
    Context, Error,
};
use ::serenity::{
    all::{Message, UserId},
    builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, EditMessage},
};
use poise::serenity_prelude as serenity;
use songbird::{
    input::{AuxMetadata, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{cmp::Ordering, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use typemap_rev::TypeMapKey;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
    DownloadMKV,
    DownloadMP3,
    Search,
}

/// Get the guild name.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    let _id = ctx.serenity_context().shard_id;
    ctx.say(format!(
        "The name of this guild is: {}",
        ctx.partial_guild().await.unwrap().name
    ))
    .await?;

    Ok(())
}

/// Sends the searching message after a play command is sent.
/// Also defers the interaction so we won't timeout.
#[cfg(not(tarpaulin_include))]
pub async fn send_search_message(ctx: Context<'_>) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{}", CrackedMessage::Search));
    send_embed_response_poise(ctx, embed).await
}

/// Play a song next
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("next", "pn", "Pn", "insert", "ins", "push")
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
#[poise::command(slash_command, prefix_command, guild_only, aliases("s", "S"))]
pub async fn search(
    ctx: Context<'_>,
    #[rest]
    #[description = "search query."]
    query: String,
) -> Result<(), Error> {
    play_internal(ctx, Some("search".to_string()), None, Some(query)).await
}

/// Play a song.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("p", "P"),
    check = "command_check_music"
)]
pub async fn play(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    query: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, None, None, query).await
}

/// Play a song with more options
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only, aliases("opt"))]
pub async fn optplay(
    ctx: Context<'_>,
    #[description = "Play mode"] mode: Option<String>,
    #[description = "File to play."] file: Option<serenity::Attachment>,
    #[rest]
    #[description = "song link or search query."]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, mode, file, query_or_url).await
}

/// Does the actual playing of the song, all the other commands use this.
#[cfg(not(tarpaulin_include))]
async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // FIXME: This should be generalized.
    let is_prefix = ctx.prefix() != "/";

    let msg = get_msg(mode.clone(), query_or_url, is_prefix);

    if msg.is_none() && file.is_none() {
        let embed = CreateEmbed::default()
            .description(format!("{}", CrackedError::Other("No query provided!")));
        send_embed_response_poise(ctx, embed).await?;
        return Ok(());
    }

    let (mode, msg) = get_mode(is_prefix, msg.clone(), mode);

    // TODO: Maybe put into it's own function?
    let url = match file.clone() {
        Some(file) => file.url.as_str().to_owned().to_string(),
        None => msg.clone(),
    };
    let url = url.as_str();

    tracing::warn!(target: "PLAY", "url: {}", url);

    // reply with a temporary message while we fetch the source
    // needed because interactions must be replied within 3s and queueing takes longer
    let mut search_msg = send_search_message(ctx).await?;

    ctx.data()
        .add_msg_to_cache(guild_id, search_msg.clone())
        .await;

    tracing::debug!("search response msg: {:?}", search_msg);

    let call = get_call_with_fail_msg(ctx).await?;

    // determine whether this is a link or a query string
    let query_type = query_type_from_url(ctx, url, file).await?;

    // FIXME: Decide whether we're using this everywhere, or not.
    // Don't like the inconsistency.
    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    // FIXME: Super hacky, fix this shit.
    // This is actually where the track gets queued into the internal queue, it's the main work function.
    let move_on = match_mode(ctx, call.clone(), mode, query_type.clone(), &mut search_msg).await?;

    // FIXME: Yeah, this is terrible, fix this.
    if !move_on {
        return Ok(());
    }

    // FIXME: What was the point of this again?
    // let _volume = {
    //     let mut settings = ctx.data().guild_settings_map.write().await; // .clone();
    //     let guild_settings = settings.entry(guild_id).or_insert_with(|| {
    //         GuildSettings::new(
    //             guild_id,
    //             Some(prefix),
    //             get_guild_name(ctx.serenity_context(), guild_id),
    //         )
    //     });
    //     guild_settings.volume
    // };

    // let queue = call.lock().await.queue().current_queue().clone();
    // tracing::warn!("guild_settings: {:?}", guild_settings);
    // refetch the queue after modification
    // FIXME: I'm beginning to think that this walking of the queue is what's causing the performance issues.
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();
    // queue.first().map(|t| t.set_volume(volume).unwrap());
    // queue.iter().for_each(|t| t.set_volume(volume).unwrap());
    drop(handler);

    // This makes sense, we're getting the final response to the user based on whether
    // the song / playlist was queued first, last, or is now playing.
    // Ah! Also, sometimes after a long queue process the now playing message says that it's already
    // X seconds into the song, so this is definitely after the section of the code that
    // takes a long time.
    let embed = match queue.len().cmp(&1) {
        Ordering::Greater => {
            let estimated_time = calculate_time_until_play(&queue, mode).await.unwrap();

            match (query_type, mode) {
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::Next,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::Next");
                    let track = queue.get(1).unwrap();
                    build_queued_embed(PLAY_TOP, track, estimated_time).await
                },
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::End,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::End");
                    let track = queue.last().unwrap();
                    build_queued_embed(PLAY_QUEUE, track, estimated_time).await
                },
                (QueryType::PlaylistLink(_) | QueryType::KeywordList(_), y) => {
                    tracing::error!(
                        "QueryType::PlaylistLink|QueryType::KeywordList, mode: {:?}",
                        y
                    );
                    CreateEmbed::default()
                        .description(format!("{}", CrackedMessage::PlaylistQueued))
                },
                (QueryType::File(_x_), y) => {
                    tracing::error!("QueryType::File, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
                (QueryType::YoutubeSearch(_x), y) => {
                    tracing::error!("QueryType::YoutubeSearch, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
                (x, y) => {
                    tracing::error!("{:?} {:?} {:?}", x, y, mode);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
            }
        },
        Ordering::Equal => {
            tracing::warn!("Only one track in queue, just playing it.");
            let track = queue.first().unwrap();
            create_now_playing_embed(track).await
        },
        Ordering::Less => {
            tracing::warn!("No tracks in queue, this only happens when an interactive search is done with an empty queue.");
            CreateEmbed::default()
                .description("No tracks in queue!")
                .footer(CreateEmbedFooter::new("No tracks in queue!"))
        },
    };

    edit_embed_response2(ctx, embed, search_msg.clone())
        .await
        .map(|_| ())
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

/// This is what actually does the majority of the work of the function.
/// It finds the track that the user wants to play and then actually
/// does the process of queuing it. This needs to be optimized.
async fn match_mode<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: QueryType,
    search_msg: &'a mut Message,
) -> Result<bool, CrackedError> {
    tracing::info!("mode: {:?}", mode);

    match mode {
        Mode::Search => query_type
            .mode_search(ctx, call)
            .await
            .map(|x| !x.is_empty()),
        Mode::DownloadMKV => query_type.mode_download(ctx, false).await,
        Mode::DownloadMP3 => query_type.mode_download(ctx, true).await,
        Mode::End => query_type.mode_end(ctx, call, search_msg).await,
        Mode::Next => query_type.mode_next(ctx, call, search_msg).await,
        Mode::Jump => query_type.mode_jump(ctx, call).await,
        Mode::All | Mode::Reverse | Mode::Shuffle => {
            query_type.mode_rest(ctx, call, search_msg).await
        },
    }
}

/// Check if the domain that we're playing from is banned.
// FIXME: This is borked.
pub fn check_banned_domains(
    guild_settings: &GuildSettings,
    query_type: Option<QueryType>,
) -> Result<Option<QueryType>, CrackedError> {
    if let Some(QueryType::Keywords(_)) = query_type {
        if !guild_settings.allow_all_domains.unwrap_or(true)
            && (guild_settings.banned_domains.contains("youtube.com")
                || (guild_settings.banned_domains.is_empty()
                    && !guild_settings.allowed_domains.contains("youtube.com")))
        {
            // let message = CrackedMessage::PlayDomainBanned {
            //     domain: "youtube.com".to_string(),
            // };

            // send_response_poise_text(ctx, message).await?;
            // Ok(None)
            Err(CrackedError::Other("youtube.com is banned"))
        } else {
            Ok(query_type)
        }
    } else {
        Ok(query_type)
    }
}

/// Calculate the time until the next track plays.
async fn calculate_time_until_play(queue: &[TrackHandle], mode: Mode) -> Option<Duration> {
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
    let metadata = get_track_metadata(top_track).await;

    let top_track_duration = match metadata.duration {
        Some(duration) => duration,
        None => return Some(Duration::MAX),
    };

    match mode {
        Mode::Next => Some(top_track_duration - top_track_elapsed),
        _ => {
            let center = &queue[1..queue.len() - 1];
            let livestreams =
                center.len() - center.iter().filter_map(|_t| metadata.duration).count();

            // if any of the tracks before are livestreams, the new track will never play
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

/// Enum for the requesting user of a track.
#[derive(Debug, Clone)]
pub enum RequestingUser {
    UserId(UserId),
}

/// We implement TypeMapKey for RequestingUser.
impl TypeMapKey for RequestingUser {
    type Value = RequestingUser;
}

/// Default implementation for RequestingUser.
impl Default for RequestingUser {
    /// Defualt is UserId(1).
    fn default() -> Self {
        let user = UserId::new(1);
        RequestingUser::UserId(user)
    }
}

/// AuxMetadata wrapper and utility functions.
#[derive(Debug, Clone)]
pub enum MyAuxMetadata {
    Data(AuxMetadata),
}

/// Implement TypeMapKey for MyAuxMetadata.
impl TypeMapKey for MyAuxMetadata {
    type Value = MyAuxMetadata;
}

/// Implement Default for MyAuxMetadata.
impl Default for MyAuxMetadata {
    fn default() -> Self {
        MyAuxMetadata::Data(AuxMetadata::default())
    }
}

/// Implement MyAuxMetadata.
impl MyAuxMetadata {
    /// Create a new MyAuxMetadata from AuxMetadata.
    pub fn new(metadata: AuxMetadata) -> Self {
        MyAuxMetadata::Data(metadata)
    }

    /// Get the internal metadata.
    pub fn metadata(&self) -> &AuxMetadata {
        match self {
            MyAuxMetadata::Data(metadata) => metadata,
        }
    }

    /// Create new MyAuxMetadata from &SpotifyTrack.
    pub fn from_spotify_track(track: &SpotifyTrack) -> Self {
        MyAuxMetadata::Data(AuxMetadata {
            track: Some(track.name()),
            artist: Some(track.artists_str()),
            album: Some(track.album_name()),
            date: None,
            start_time: Some(Duration::ZERO),
            duration: Some(track.duration()),
            channels: Some(2),
            channel: None,
            sample_rate: None,
            source_url: None,
            thumbnail: Some(track.name()),
            title: Some(track.name()),
        })
    }

    /// Set the source_url.
    pub fn with_source_url(self, source_url: String) -> Self {
        MyAuxMetadata::Data(AuxMetadata {
            source_url: Some(source_url),
            ..self.metadata().clone()
        })
    }

    /// Get a search query from the metadata for youtube.
    pub fn get_search_query(&self) -> String {
        let metadata = self.metadata();
        let title = metadata.title.clone().unwrap_or_default();
        let artist = metadata.artist.clone().unwrap_or_default();
        format!("{} {}", title, artist)
    }
}

/// Implementation to convert `[&SpotifyTrack]` to `[MyAuxMetadata]`.
impl From<&SpotifyTrack> for MyAuxMetadata {
    fn from(track: &SpotifyTrack) -> Self {
        MyAuxMetadata::from_spotify_track(track)
    }
}

/// Build an embed for the cure
async fn build_queued_embed(
    author_title: &str,
    track: &TrackHandle,
    estimated_time: Duration,
) -> CreateEmbed {
    // FIXME
    let metadata = {
        let map = track.typemap().read().await;
        let my_metadata = map.get::<MyAuxMetadata>().unwrap();

        match my_metadata {
            MyAuxMetadata::Data(metadata) => metadata.clone(),
        }
    };
    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();
    let meta_title = metadata.title.clone().unwrap_or(QUEUE_NO_TITLE.to_string());
    let source_url = metadata
        .source_url
        .clone()
        .unwrap_or(QUEUE_NO_SRC.to_string());

    // let title_text = &format!("[**{}**]({})", meta_title, source_url);

    let footer_text = format!(
        "{}{}\n{}{}",
        TRACK_DURATION,
        get_human_readable_timestamp(metadata.duration),
        TRACK_TIME_TO_PLAY,
        get_human_readable_timestamp(Some(estimated_time))
    );

    let author = CreateEmbedAuthor::new(author_title);

    CreateEmbed::new()
        .author(author)
        .title(meta_title)
        .url(source_url)
        .thumbnail(thumbnail)
        .footer(CreateEmbedFooter::new(footer_text))
}

/// Add tracks to the queue from aux_metadata.
#[cfg(not(tarpaulin_include))]
pub async fn queue_aux_metadata(
    ctx: Context<'_>,
    aux_metadata: &[MyAuxMetadata],
    mut msg: Message,
) -> Result<(), CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let search_results = aux_metadata;

    let client = &ctx.data().http_client;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    for metadata in search_results {
        let source_url = metadata.metadata().source_url.as_ref();
        let metadata_final = if source_url.is_none() || source_url.unwrap().is_empty() {
            let search_query = build_query_aux_metadata(metadata.metadata());
            let _ = msg
                .edit(
                    ctx,
                    EditMessage::default().content(format!("Queuing... {}", search_query)),
                )
                .await;

            let ytdl = RustyYoutubeClient::new_with_client(client.clone())?;
            let res = ytdl.one_shot(search_query).await?;
            let res = res.ok_or(CrackedError::Other("No results found"))?;
            let new_aux_metadata = RustyYoutubeClient::search_result_to_aux_metadata(&res);

            MyAuxMetadata::Data(new_aux_metadata)
        } else {
            metadata.clone()
        };

        let ytdl = YoutubeDl::new(
            client.clone(),
            metadata_final.metadata().source_url.clone().unwrap(),
        );

        let query_type = QueryType::NewYoutubeDl((ytdl, metadata_final.metadata().clone()));
        let _ = queue_track_back(ctx, &call, &query_type).await?;
    }

    let queue = call.lock().await.queue().current_queue();
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    Ok(())
}
