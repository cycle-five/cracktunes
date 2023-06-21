use self::serenity::{builder::CreateEmbed, Mutex};
use crate::{
    commands::skip::force_skip_top_track,
    errors::{verify, CrackedError},
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::track_end::update_queue_messages,
    messaging::message::ParrotMessage,
    messaging::messages::{
        PLAY_QUEUE, PLAY_TOP, SPOTIFY_AUTH_FAILED, TRACK_DURATION, TRACK_TIME_TO_PLAY,
    },
    sources::{
        spotify::{Spotify, SPOTIFY},
        youtube::{YouTube, YouTubeRestartable},
    },
    utils::{
        compare_domains, create_embed_response_poise, create_now_playing_embed,
        create_response_poise_text, edit_embed_response_poise, edit_response_poise,
        get_human_readable_timestamp, get_interaction, summon_short,
    },
    Context, Error,
};
use poise::serenity_prelude::{self as serenity};
use songbird::{input::Restartable, tracks::TrackHandle, Call};
use std::{cmp::Ordering, error::Error as StdError, sync::Arc, time::Duration};
use url::Url;

#[derive(Clone, Copy)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
}

#[derive(Clone)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    PlaylistLink(String),
}

/// Get the guild name (guild-only)
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn get_guild_name(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(format!(
        "The name of this guild is: {}",
        ctx.partial_guild().await.unwrap().name
    ))
    .await?;

    Ok(())
}
// while let Some(mci) = serenity::CollectComponentInteraction::new(ctx)
//     .author_id(ctx.author().id)
//     .channel_id(ctx.channel_id())
//     .timeout(std::time::Duration::from_secs(120))
//     .filter(move |mci| mci.data.custom_id == uuid_boop.to_string())
//     .await

#[poise::command(slash_command, prefix_command)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Play mode"] mode: Option<String>,
    #[rest]
    #[description = "song link or search query."]
    msg: Option<String>,
) -> Result<(), Error> {
    tracing::info!(target: "commands", "play called {}", msg.as_deref().unwrap_or("nothing"));

    if msg.is_none() {
        let mut embed = CreateEmbed::default();
        embed.description(format!("{}", CrackedError::Other("No query provided!")));
        create_embed_response_poise(ctx, embed).await?;
        return Ok(());
    }

    let mode = match mode.unwrap_or("next".to_string()).as_str() {
        "next" => Mode::Next,
        "all" => Mode::All,
        "reverse" => Mode::Reverse,
        "shuffle" => Mode::Shuffle,
        "jump" => Mode::Jump,
        _ => Mode::End,
    };

    let msg = msg.unwrap();
    let url = msg.as_str();

    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    // let guild_id = interaction.guild_id.unwrap();
    // let manager = songbird::get(ctx).await.unwrap();

    // // try to join a voice channel if not in one just yet
    // summon().slash_action.unwrap()(&ctx.clone()).await;
    match summon_short(ctx).await {
        Ok(_) => {}
        Err(_) => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };
    // let call = manager.get(guild_id).unwrap();

    // determine whether this is a link or a query string
    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") => {
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, url).await?)
            }
            Some(other) => {
                let mut data = ctx.serenity_context().data.write().await;
                let settings = data.get_mut::<GuildSettingsMap>().unwrap();
                let guild_settings = settings
                    .entry(guild_id)
                    .or_insert_with(|| GuildSettings::new(guild_id));

                let is_allowed = guild_settings
                    .allowed_domains
                    .iter()
                    .any(|d| compare_domains(d, other));

                let is_banned = guild_settings
                    .banned_domains
                    .iter()
                    .any(|d| compare_domains(d, other));

                if is_banned || (guild_settings.banned_domains.is_empty() && !is_allowed) {
                    let message = ParrotMessage::PlayDomainBanned {
                        domain: other.to_string(),
                    };

                    return create_response_poise_text(&ctx, message).await;
                }

                YouTube::extract(url)
            }
            None => None,
        },
        Err(_) => {
            let mut data = ctx.serenity_context().data.write().await;
            let settings = data.get_mut::<GuildSettingsMap>().unwrap();
            let guild_settings = settings
                .entry(guild_id)
                .or_insert_with(|| GuildSettings::new(guild_id));

            if guild_settings.banned_domains.contains("youtube.com")
                || (guild_settings.banned_domains.is_empty()
                    && !guild_settings.allowed_domains.contains("youtube.com"))
            {
                let message = ParrotMessage::PlayDomainBanned {
                    domain: "youtube.com".to_string(),
                };

                return create_response_poise_text(&ctx, message).await;
            }

            Some(QueryType::Keywords(url.to_string()))
        }
    };

    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    // reply with a temporary message while we fetch the source
    // needed because interactions must be replied within 3s and queueing takes longer
    create_response_poise_text(&ctx, ParrotMessage::Search).await?;

    let handler = call.lock().await;
    let queue_was_empty = handler.queue().is_empty();
    drop(handler);

    match mode {
        Mode::End => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) => {
                let queue = enqueue_track(&call, &query_type).await?;
                update_queue_messages(
                    &ctx.serenity_context().http,
                    &ctx.serenity_context().data,
                    &queue,
                    guild_id,
                )
                .await;
            }
            QueryType::PlaylistLink(url) => {
                let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                    .await
                    .ok_or(CrackedError::Other("failed to fetch playlist"))?;

                for url in urls.iter() {
                    let queue =
                        enqueue_track(&call, &QueryType::VideoLink(url.to_string())).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                for keywords in keywords_list.iter() {
                    let queue =
                        enqueue_track(&call, &QueryType::Keywords(keywords.to_string())).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
        },
        Mode::Next => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) => {
                let queue = insert_track(&call, &query_type, 1).await?;
                update_queue_messages(
                    &ctx.serenity_context().http,
                    &ctx.serenity_context().data,
                    &queue,
                    guild_id,
                )
                .await;
            }
            QueryType::PlaylistLink(url) => {
                let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                    .await
                    .ok_or(CrackedError::Other("failed to fetch playlist"))?;

                for (idx, url) in urls.into_iter().enumerate() {
                    let queue = insert_track(&call, &QueryType::VideoLink(url), idx + 1).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                for (idx, keywords) in keywords_list.into_iter().enumerate() {
                    let queue =
                        insert_track(&call, &QueryType::Keywords(keywords), idx + 1).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
        },
        Mode::Jump => match query_type.clone() {
            QueryType::Keywords(_) | QueryType::VideoLink(_) => {
                let mut queue = enqueue_track(&call, &query_type).await?;

                if !queue_was_empty {
                    rotate_tracks(&call, 1).await.ok();
                    queue = force_skip_top_track(&call.lock().await).await?;
                }

                update_queue_messages(
                    &ctx.serenity_context().http,
                    &ctx.serenity_context().data,
                    &queue,
                    guild_id,
                )
                .await;
            }
            QueryType::PlaylistLink(url) => {
                let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                    .await
                    .ok_or(CrackedError::Other("failed to fetch playlist"))?;

                let mut insert_idx = 1;

                for (i, url) in urls.into_iter().enumerate() {
                    let mut queue =
                        insert_track(&call, &QueryType::VideoLink(url), insert_idx).await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                let mut insert_idx = 1;

                for (i, keywords) in keywords_list.into_iter().enumerate() {
                    let mut queue =
                        insert_track(&call, &QueryType::Keywords(keywords), insert_idx).await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
        },
        Mode::All | Mode::Reverse | Mode::Shuffle => match query_type.clone() {
            QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => {
                let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                    .await
                    .ok_or(CrackedError::Other("failed to fetch playlist"))?;

                for url in urls.into_iter() {
                    let queue = enqueue_track(&call, &QueryType::VideoLink(url)).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            QueryType::KeywordList(keywords_list) => {
                for keywords in keywords_list.into_iter() {
                    let queue = enqueue_track(&call, &QueryType::Keywords(keywords)).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        &ctx.serenity_context().data,
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            }
            _ => {
                ctx.defer().await?;
                edit_response_poise(ctx, ParrotMessage::PlayAllFailed).await?;
                //edit_response(&ctx.http, interaction, ParrotMessage::PlayAllFailed).await?;
                return Ok(());
            }
        },
    }
    let handler = call.lock().await;

    let mut data = ctx.serenity_context().data.write().await;
    let settings = data.get_mut::<GuildSettingsMap>().unwrap();
    let guild_settings = settings
        .entry(guild_id)
        .or_insert_with(|| GuildSettings::new(guild_id));

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    queue
        .iter()
        .for_each(|t| t.set_volume(guild_settings.volume).unwrap());
    drop(data);
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
                    //FIXME: asdfasdf
                    get_interaction(ctx)
                        .unwrap()
                        .edit_original_interaction_response(
                            &ctx.serenity_context().http,
                            |message| message.content(ParrotMessage::PlaylistQueued),
                        )
                        .await?;
                    edit_response_poise(ctx, ParrotMessage::PlaylistQueued).await?;
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

    embed.thumbnail(&metadata.thumbnail.unwrap());

    embed.field(
        title,
        &format!(
            "[**{}**]({})",
            metadata.title.unwrap(),
            metadata.source_url.unwrap()
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

async fn get_track_source(query_type: QueryType) -> Result<Restartable, CrackedError> {
    match query_type {
        QueryType::VideoLink(query) => YouTubeRestartable::ytdl(query, true)
            .await
            .map_err(CrackedError::TrackFail),

        QueryType::Keywords(query) => YouTubeRestartable::ytdl_search(query, true)
            .await
            .map_err(CrackedError::TrackFail),

        _ => unreachable!(),
    }
}

async fn enqueue_track(
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let source = get_track_source(query_type.clone()).await?;

    let mut handler = call.lock().await;
    handler.enqueue_source(source.into());

    Ok(handler.queue().current_queue())
}

async fn insert_track(
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
    idx: usize,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let handler = call.lock().await;
    let queue_size = handler.queue().len();
    drop(handler);

    if queue_size <= 1 {
        let queue = enqueue_track(call, query_type).await?;
        return Ok(queue);
    }

    verify(
        idx > 0 && idx <= queue_size,
        CrackedError::NotInRange("index", idx as isize, 1, queue_size as isize),
    )?;

    enqueue_track(call, query_type).await?;

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
