use self::serenity::builder::CreateEmbed;
use crate::{
    commands::skip::force_skip_top_track,
    errors::{verify, CrackedError},
    guild::settings::GuildSettings,
    handlers::track_end::update_queue_messages,
    http_utils,
    messaging::message::CrackedMessage,
    messaging::messages::{
        PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, SPOTIFY_AUTH_FAILED, TRACK_DURATION,
        TRACK_TIME_TO_PLAY,
    },
    sources::{
        file::FileRestartable,
        spotify::{Spotify, SPOTIFY},
        // youtube::{YouTube, YouTubeRestartable},
    },
    utils::{
        compare_domains, create_embed_response_poise, create_now_playing_embed,
        create_response_poise_text, edit_embed_response_poise, edit_response_poise, get_guild_name,
        get_human_readable_timestamp, get_interaction,
    },
    Context, Error,
};
use poise::serenity_prelude::{self as serenity, Attachment, Http};
use songbird::{
    input::{File as FileInput, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{cmp::Ordering, error::Error as StdError, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use url::Url;

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
}

#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    PlaylistLink(String),
    File(serenity::Attachment),
}

/// Get the guild name (guild-only)
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(format!(
        "The name of this guild is: {}",
        ctx.partial_guild().await.unwrap().name
    ))
    .await?;

    Ok(())
}

/// Play a song.
#[poise::command(slash_command, prefix_command, guild_only, aliases("p", "P"))]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Play mode"] mode: Option<String>,
    #[description = "File to play."] file: Option<serenity::Attachment>,
    #[rest]
    #[description = "song link or search query."]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    let prefix = ctx.prefix();
    let is_prefix = ctx.prefix() != "/";
    let mut msg = query_or_url.clone().map(|s| s.replace("query_or_url:", ""));

    if is_prefix {
        msg = Some(
            mode.clone()
                .map(|s| s.replace("query_or_url:", ""))
                .unwrap_or("".to_string())
                + " "
                + &msg.unwrap_or("".to_string()),
        );
    }

    if msg.is_none() && file.is_none() {
        let mut embed = CreateEmbed::default();
        embed.description(format!("{}", CrackedError::Other("No query provided!")));
        create_embed_response_poise(ctx, embed).await?;
        return Ok(());
    }

    let mode = if is_prefix {
        match msg
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or_default()
        {
            "next:" => Mode::Next,
            "all:" => Mode::All,
            "reverse:" => Mode::Reverse,
            "shuffle:" => Mode::Shuffle,
            "jump:" => Mode::Jump,
            _ => Mode::End,
        }
    } else {
        match mode
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default()
            .as_str()
        {
            "next" => Mode::Next,
            "all" => Mode::All,
            "reverse" => Mode::Reverse,
            "shuffle" => Mode::Shuffle,
            "jump" => Mode::Jump,
            _ => Mode::End,
        }
    };

    let url = match file.clone() {
        Some(file) => file.url.as_str().to_owned().to_string(),
        None => msg.unwrap(),
    };
    let url = url.as_str();

    tracing::warn!(target: "PLAY", "url: {}", url);

    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NoGuildId));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            // try to join a voice channel if not in one just yet
            //match summon_short(ctx).await {
            match manager.join(guild_id, ctx.channel_id()).await.1 {
                Ok(_) => manager.get(guild_id).unwrap(),
                Err(_) => {
                    let mut embed = CreateEmbed::default();
                    embed.description(format!("{}", CrackedError::NotConnected));
                    create_embed_response_poise(ctx, embed).await?;
                    return Ok(());
                }
            }
        }
    };

    // determine whether this is a link or a query string
    let query_type = match_url(&ctx, url, file).await?;

    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    // reply with a temporary message while we fetch the source
    // needed because interactions must be replied within 3s and queueing takes longer
    match get_interaction(ctx) {
        Some(interaction) => {
            interaction.defer(ctx.http()).await.unwrap();
            interaction
                .create_interaction_response(&ctx.serenity_context().http, |response| {
                    response.kind(serenity::MessageInteraction::InteractionType::Message)
                })
                .await?;
        }
        None => create_response_poise_text(&ctx, CrackedMessage::Search).await?,
    };

    match_mode(&ctx, call.clone(), mode, query_type.clone()).await?;

    let handler = call.lock().await;

    let mut settings = ctx.data().guild_settings_map.lock().unwrap().clone();
    let guild_settings = settings.entry(guild_id).or_insert_with(|| {
        GuildSettings::new(
            guild_id,
            Some(prefix),
            get_guild_name(ctx.serenity_context(), guild_id),
        )
    });

    tracing::warn!("guild_settings: {:?}", guild_settings);
    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    queue
        .iter()
        .for_each(|t| t.set_volume(guild_settings.volume).unwrap());
    drop(handler);

    match queue.len().cmp(&1) {
        Ordering::Greater => {
            let estimated_time = calculate_time_until_play(&queue, mode).await.unwrap();

            match (query_type, mode) {
                (QueryType::VideoLink(_) | QueryType::Keywords(_), Mode::Next) => {
                    let track = queue.get(1).unwrap();
                    let embed = create_queued_embed(PLAY_TOP, track, estimated_time).await;

                    edit_embed_response_poise(ctx, embed).await?;
                }
                (QueryType::VideoLink(_) | QueryType::Keywords(_), Mode::End) => {
                    let track = queue.last().unwrap();
                    let embed = create_queued_embed(PLAY_QUEUE, track, estimated_time).await;

                    edit_embed_response_poise(ctx, embed).await?;
                }
                (QueryType::PlaylistLink(_) | QueryType::KeywordList(_), _) => {
                    match get_interaction(ctx) {
                        Some(interaction) => {
                            interaction
                                .edit_original_interaction_response(
                                    &ctx.serenity_context().http,
                                    |message| message.content(CrackedMessage::PlaylistQueued),
                                )
                                .await?;
                        }
                        None => {
                            edit_response_poise(ctx, CrackedMessage::PlaylistQueued).await?;
                        }
                    }
                }
                (_, _) => {}
            }
        }
        Ordering::Equal => {
            let track = queue.first().unwrap();
            let embed = create_now_playing_embed(track).await;

            edit_embed_response_poise(ctx, embed).await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

async fn match_mode(
    ctx: &Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: QueryType,
) -> Result<(), Error> {
    let handler = call.lock().await;
    let queue_was_empty = handler.queue().is_empty();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    drop(handler);

    tracing::info!("mode: {:?}", mode);

    match mode {
        Mode::End => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) => {
                tracing::trace!("Mode::End, QueryType::Keywords | QueryType::VideoLink");
                let queue = enqueue_track(ctx.http(), &call, &query_type).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            }
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::End, QueryType::PlaylistLink");
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::PlayListFail)?;
                let urls = vec!["".to_string()];

                for url in urls.iter() {
                    let queue =
                        enqueue_track(ctx.http(), &call, &QueryType::VideoLink(url.to_string()))
                            .await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::End, QueryType::KeywordList");
                for keywords in keywords_list.iter() {
                    let queue = enqueue_track(
                        ctx.http(),
                        &call,
                        &QueryType::Keywords(keywords.to_string()),
                    )
                    .await?;
                    update_queue_messages(&ctx.http(), ctx.data(), &queue, guild_id).await;
                }
            }
            QueryType::File(file) => {
                tracing::trace!("Mode::End, QueryType::File");
                let queue = //ffmpeg::from_attachment(file, Metadata::default(), &[]).await?;
                        enqueue_track(ctx.http(), &call, &QueryType::File(file)).await?;
                update_queue_messages(&ctx.http(), ctx.data(), &queue, guild_id).await;
            }
        },
        Mode::Next => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::File(_) => {
                tracing::trace!(
                    "Mode::Next, QueryType::Keywords | QueryType::VideoLink | QueryType::File"
                );
                let queue = insert_track(ctx.http(), &call, &query_type, 1).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            }
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::Next, QueryType::PlaylistLink");
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::Other("failed to fetch playlist"))?;
                let urls = vec!["".to_string()];

                for (idx, url) in urls.into_iter().enumerate() {
                    let queue =
                        insert_track(ctx.http(), &call, &QueryType::VideoLink(url), idx + 1)
                            .await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::Next, QueryType::KeywordList");
                let q_not_empty = if call.clone().lock().await.queue().is_empty() {
                    0
                } else {
                    1
                };
                for (idx, keywords) in keywords_list.into_iter().enumerate() {
                    let queue = insert_track(
                        ctx.http(),
                        &call,
                        &QueryType::Keywords(keywords),
                        idx + q_not_empty,
                    )
                    .await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
        },
        Mode::Jump => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::File(_) => {
                tracing::trace!(
                    "Mode::Jump, QueryType::Keywords | QueryType::VideoLink | QueryType::File"
                );
                let mut queue = enqueue_track(ctx.http(), &call, &query_type).await?;

                if !queue_was_empty {
                    rotate_tracks(&call, 1).await.ok();
                    queue = force_skip_top_track(&call.lock().await).await?;
                }

                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            }
            QueryType::PlaylistLink(url) => {
                tracing::error!("Mode::Jump, QueryType::PlaylistLink");
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::PlayListFail)?;
                let _src = YoutubeDl::new(ctx.serenity_context().http, url);
                // .ok_or(CrackedError::Other("failed to fetch playlist"))?
                // .into_iter()
                // .for_each(|track| async {
                //     let _ = enqueue_track(&call, &QueryType::File(track)).await;
                // });
                let urls = vec!["".to_string()];
                let mut insert_idx = 1;

                for (i, url) in urls.into_iter().enumerate() {
                    let mut queue =
                        insert_track(ctx.http(), &call, &QueryType::VideoLink(url), insert_idx)
                            .await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                tracing::error!("Mode::Jump, QueryType::KeywordList");
                let mut insert_idx = 1;

                for (i, keywords) in keywords_list.into_iter().enumerate() {
                    let mut queue = insert_track(
                        ctx.http(),
                        &call,
                        &QueryType::Keywords(keywords),
                        insert_idx,
                    )
                    .await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
        },
        Mode::All | Mode::Reverse | Mode::Shuffle => match query_type.clone() {
            QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::VideoLink | QueryType::PlaylistLink");
                YoutubeDl::ytdl(url)
                    .await
                    .ok_or(CrackedError::Other("failed to fetch playlist"))?
                    .into_iter()
                    .for_each(|track| async {
                        let _ = enqueue_track(ctx.http(), &call, &QueryType::File(track)).await;
                    });
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::PlayListFail)?;

                // for url in urls.into_iter() {
                //     let queue = enqueue_track(&call, &QueryType::VideoLink(url)).await?;
                //     update_queue_messages(
                //         &ctx.serenity_context().http,
                //         ctx.data(),
                //         &queue,
                //         guild_id,
                //     )
                //     .await;
                // }
            }
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!(
                    "Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::KeywordList"
                );
                for keywords in keywords_list.into_iter() {
                    let queue = enqueue_track(
                        ctx.serenity_context().http,
                        &call,
                        &QueryType::Keywords(keywords),
                    )
                    .await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            _ => {
                ctx.defer().await?; // Why did I do this?
                edit_response_poise(*ctx, CrackedMessage::PlayAllFailed).await?;
                return Ok(());
            }
        },
    }

    Ok(())
}

use colored::Colorize;
/// Matches a url (or query string) to a QueryType
async fn match_url(
    ctx: &Context<'_>,
    url: &str,
    file: Option<Attachment>,
) -> Result<Option<QueryType>, Error> {
    // determine whether this is a link or a query string
    tracing::warn!("url: {}", url);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") | Some("spotify.link") => {
                let final_url = http_utils::resolve_final_url(url).await?;
                tracing::error!("spotify: {} -> {}", url, final_url);
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, &final_url).await?)
            }
            Some("cdn.discordapp.com") => {
                tracing::warn!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            }
            Some(other) => {
                let mut settings = ctx.data().guild_settings_map.lock().unwrap().clone();
                let guild_settings = settings.entry(guild_id).or_insert_with(|| {
                    GuildSettings::new(
                        guild_id,
                        Some(ctx.prefix()),
                        get_guild_name(ctx.serenity_context(), guild_id),
                    )
                });
                if !guild_settings.allow_all_domains.unwrap_or(true) {
                    let is_allowed = guild_settings
                        .allowed_domains
                        .iter()
                        .any(|d| compare_domains(d, other));

                    let is_banned = guild_settings
                        .banned_domains
                        .iter()
                        .any(|d| compare_domains(d, other));

                    if is_banned || (guild_settings.banned_domains.is_empty() && !is_allowed) {
                        let message = CrackedMessage::PlayDomainBanned {
                            domain: other.to_string(),
                        };

                        create_response_poise_text(ctx, message).await?;
                    }
                }

                //YouTube::extract(url)
                YoutubeDl::new(ctx.serenity_context().http, url.to_string())
            }
            None => None,
        },
        Err(_) => {
            let mut settings = ctx.data().guild_settings_map.lock().unwrap().clone();
            let guild_settings = settings.entry(guild_id).or_insert_with(|| {
                GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id),
                )
            });
            if !guild_settings.allow_all_domains.unwrap_or(true)
                && (guild_settings.banned_domains.contains("youtube.com")
                    || (guild_settings.banned_domains.is_empty()
                        && !guild_settings.allowed_domains.contains("youtube.com")))
            {
                let message = CrackedMessage::PlayDomainBanned {
                    domain: "youtube.com".to_string(),
                };

                create_response_poise_text(ctx, message).await?;
            }

            Some(QueryType::Keywords(url.to_string()))
        }
    };

    Result::Ok(query_type)
}

async fn calculate_time_until_play(queue: &[TrackHandle], mode: Mode) -> Option<Duration> {
    if queue.is_empty() {
        return None;
    }

    let top_track = queue.first()?;
    let top_track_elapsed = top_track.get_info().await.unwrap().position;

    let top_track_duration = match top_track.metadata().duration {
        Some(duration) => duration,
        None => return Some(Duration::MAX),
    };

    match mode {
        Mode::Next => Some(top_track_duration - top_track_elapsed),
        _ => {
            let center = &queue[1..queue.len() - 1];
            let livestreams =
                center.len() - center.iter().filter_map(|t| t.metadata().duration).count();

            // if any of the tracks before are livestreams, the new track will never play
            if livestreams > 0 {
                return Some(Duration::MAX);
            }

            let durations = center.iter().fold(Duration::ZERO, |acc, x| {
                acc + x.metadata().duration.unwrap()
            });

            Some(durations + top_track_duration - top_track_elapsed)
        }
    }
}

async fn create_queued_embed(
    title: &str,
    track: &TrackHandle,
    estimated_time: Duration,
) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let metadata = track.metadata().clone();

    embed.thumbnail(&metadata.thumbnail.unwrap_or_default());

    embed.field(
        title,
        &format!(
            "[**{}**]({})",
            metadata.title.unwrap_or(QUEUE_NO_TITLE.to_string()),
            metadata.source_url.unwrap_or(QUEUE_NO_SRC.to_string())
        ),
        false,
    );

    let footer_text = format!(
        "{}{}\n{}{}",
        TRACK_DURATION,
        get_human_readable_timestamp(metadata.duration),
        TRACK_TIME_TO_PLAY,
        get_human_readable_timestamp(Some(estimated_time))
    );

    embed.footer(|footer| footer.text(footer_text));
    embed
}

async fn get_track_source(http: Http, query_type: QueryType) -> YoutubeDl {
    match query_type {
        QueryType::VideoLink(query) => YoutubeDl::new(http, query),
        QueryType::Keywords(query) => {
            YoutubeDl::new(http, query)
            // YouTubeRestartable::ytdl_search(query, true)
            //     .await
            //     .map_err(|e| {
            //         tracing::error!("error: {}", e);
            //         e.into()
            //     })
        }
        QueryType::File(file) => {
            FileInput::new(file.url.to_owned())
            // FileRestartable::download(file.url.to_owned(), true)
            // .await
            // .map_err(|e| {
            //     tracing::error!("error: {}", e);
            //     e.into()
            // }),
        }
        _ => unreachable!(),
    }
}

async fn enqueue_track(
    http: Http,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let source = get_track_source(http, query_type.clone()).into();

    let mut handler = call.lock().await;
    handler.enqueue_source(source.into());

    Ok(handler.queue().current_queue())
}

async fn insert_track(
    http: Http,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
    idx: usize,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let handler = call.lock().await;
    let queue_size = handler.queue().len();
    drop(handler);
    tracing::trace!("queue_size: {}, idx: {}", queue_size, idx);

    if queue_size <= 1 {
        let queue = enqueue_track(http, call, query_type).await?;
        return Ok(queue);
    }

    verify(
        idx > 0 && idx <= queue_size + 1,
        CrackedError::NotInRange("index", idx as isize, 1, queue_size as isize),
    )?;

    enqueue_track(http, call, query_type).await?;

    let handler = call.lock().await;
    handler.queue().modify_queue(|queue| {
        let back = queue.pop_back().unwrap();
        queue.insert(idx, back);
    });

    Ok(handler.queue().current_queue())
}

async fn rotate_tracks(
    call: &Arc<Mutex<Call>>,
    n: usize,
) -> Result<Vec<TrackHandle>, Box<dyn StdError>> {
    let handler = call.lock().await;

    verify(
        handler.queue().len() > 2,
        CrackedError::Other("cannot rotate queues smaller than 3 tracks"),
    )?;

    handler.queue().modify_queue(|queue| {
        let mut not_playing = queue.split_off(1);
        not_playing.rotate_right(n);
        queue.append(&mut not_playing);
    });

    Ok(handler.queue().current_queue())
}
