use self::serenity::builder::CreateEmbed;
use crate::{
    commands::{aux_metadata_to_db_structures, skip::force_skip_top_track},
    connection::get_voice_channel_for_user,
    errors::{verify, CrackedError},
    guild::settings::{GuildSettings, DEFAULT_PREMIUM},
    handlers::track_end::update_queue_messages,
    http_utils,
    messaging::message::CrackedMessage,
    messaging::messages::{
        PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, SPOTIFY_AUTH_FAILED, TRACK_DURATION,
        TRACK_TIME_TO_PLAY,
    },
    sources::spotify::{Spotify, SPOTIFY},
    utils::{
        compare_domains, create_now_playing_embed, edit_response_poise, get_guild_name,
        get_human_readable_timestamp, get_interaction, get_track_metadata,
        send_embed_response_poise, send_response_poise_text,
    },
    Context, Error,
};
use ::serenity::{
    all::{
        ChannelId, ComponentInteractionDataKind, Context as SerenityContext, EmbedField, GuildId,
        Mentionable, Message, UserId,
    },
    builder::{
        CreateAttachment, CreateEmbedAuthor, CreateEmbedFooter, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, EditInteractionResponse, EditMessage,
    },
};
use poise::serenity_prelude::{self as serenity, Attachment, Http};
use reqwest::Client;
use songbird::{
    input::{AuxMetadata, Compose, HttpRequest, Input as SongbirdInput, YoutubeDl},
    tracks::{Track, TrackHandle},
    Call,
};
use sqlx::PgPool;
use std::{
    cmp::{min, Ordering},
    collections::HashMap,
    error::Error as StdError,
    path::Path,
    process::Output,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use typemap_rev::TypeMapKey;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
    Download,
    Search,
}

#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    PlaylistLink(String),
    File(serenity::Attachment),
    NewYoutubeDl((YoutubeDl, AuxMetadata)),
    YoutubeSearch(String),
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

fn get_mode(is_prefix: bool, msg: Option<String>, mode: Option<String>) -> (Mode, String) {
    let opt_mode = mode.clone();
    if is_prefix {
        let asdf2 = msg
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default();
        let asdf = asdf2.split_whitespace().next().unwrap_or_default();
        let mode = if asdf.starts_with("next") {
            Mode::Next
        } else if asdf.starts_with("all") {
            Mode::All
        } else if asdf.starts_with("shuffle") {
            Mode::Shuffle
        } else if asdf.starts_with("reverse") {
            Mode::Reverse
        } else if asdf.starts_with("jump") {
            Mode::Jump
        } else if asdf.starts_with("download") {
            Mode::Download
        } else if asdf.starts_with("search") {
            Mode::Search
        } else {
            Mode::End
        };
        if mode != Mode::End {
            let s = msg.clone().unwrap_or_default();
            let s2 = s.splitn(2, char::is_whitespace).last().unwrap();
            (mode, s2.to_string())
        } else {
            (Mode::End, msg.unwrap_or_default())
        }
    } else {
        let mode = match opt_mode
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
            "download" => Mode::Download,
            "search" => Mode::Search,
            _ => Mode::End,
        };
        (mode, msg.unwrap_or_default())
    }
}

/// Parses the msg variable from the parameters to the play command which due
/// to the way that the way the poise library works with auto filling them
/// based on types, it could be kind of mangled if the prefix version of the
/// command is used.
fn get_msg(mode: Option<String>, query_or_url: Option<String>, is_prefix: bool) -> Option<String> {
    let step1 = query_or_url.clone().map(|s| s.replace("query_or_url:", ""));
    if is_prefix {
        match (mode
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or("".to_string())
            + " "
            + &step1.unwrap_or("".to_string()))
            .trim()
        {
            "" => None,
            x => Some(x.to_string()),
        }
    } else {
        step1
    }
}

/// Get the call handle for songbird
/// FIXME: Does this need to take the GuildId?
#[inline]
pub async fn get_call_with_fail_msg(
    ctx: Context<'_>,
    guild_id: serenity::GuildId,
) -> Result<Arc<Mutex<Call>>, Error> {
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    match manager.get(guild_id) {
        Some(call) => Ok(call),
        None => {
            // try to join a voice channel if not in one just yet
            //match summon_short(ctx).await {
            let channel_id =
                get_voice_channel_for_user(&ctx.guild().unwrap().clone(), &ctx.author().id);
            match manager.join(guild_id, channel_id.unwrap()).await {
                Ok(_) => Ok(manager.get(guild_id).unwrap()),
                Err(_) => {
                    let embed = CreateEmbed::default()
                        .description(format!("{}", CrackedError::NotConnected));
                    send_embed_response_poise(ctx, embed).await?;
                    Err(CrackedError::NotConnected.into())
                }
            }
        }
    }
}

/// Sends the searching message after a play command is sent.
/// Also defers the interaction so we won't timeout.
async fn send_search_message(ctx: Context<'_>) -> Result<Message, Error> {
    let embed = CreateEmbed::default().description(format!("{}", CrackedMessage::Search));
    send_embed_response_poise(ctx, embed).await
    // match get_interaction_new(ctx) {
    //     Some(CommandOrMessageInteraction::Command(interaction)) => {
    //         create_response_interaction(
    //             &ctx.serenity_context().http,
    //             &interaction,
    //             CrackedMessage::Search.into(),
    //             true,
    //         )
    //         .await
    //     }
    //     _ => send_response_poise_text(ctx, CrackedMessage::Search).await,
    // }
    //Err(CrackedError::Other("Failed to send search message.").into())
}

async fn get_guild_id_with_fail_msg(ctx: Context<'_>) -> Result<serenity::GuildId, Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    Ok(guild_id)
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

    let guild_id = get_guild_id_with_fail_msg(ctx).await?;

    let call = get_call_with_fail_msg(ctx, guild_id).await?;

    // determine whether this is a link or a query string
    let query_type = get_query_type_from_url(ctx, url, file).await?;

    // FIXME: Decide whether we're using this everywhere, or not.
    // Don't like the inconsistency.
    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    // reply with a temporary message while we fetch the source
    // needed because interactions must be replied within 3s and queueing takes longer
    let msg = send_search_message(ctx).await?;

    // FIXME: Super hacky, fix this shit.
    let move_on = match_mode(ctx, call.clone(), mode, query_type.clone()).await?;

    if !move_on {
        return Ok(());
    }

    let handler = call.lock().await;

    let mut settings = ctx.data().guild_settings_map.write().unwrap().clone();
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
                }
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::End,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::End");
                    let track = queue.last().unwrap();
                    build_queued_embed(PLAY_QUEUE, track, estimated_time).await
                }
                (QueryType::PlaylistLink(_) | QueryType::KeywordList(_), y) => {
                    tracing::error!(
                        "QueryType::PlaylistLink|QueryType::KeywordList, mode: {:?}",
                        y
                    );
                    CreateEmbed::default()
                        .description(format!("{}", CrackedMessage::PlaylistQueued))
                }
                (QueryType::File(_x_), y) => {
                    tracing::error!("QueryType::File, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                }
                (QueryType::YoutubeSearch(_x), y) => {
                    tracing::error!("QueryType::YoutubeSearch, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                }
                (x, y) => {
                    tracing::error!("{:?} {:?} {:?}", x, y, mode);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                }
            }
        }
        Ordering::Equal => {
            tracing::warn!("Only one track in queue, just playing it.");
            let track = queue.first().unwrap();
            let embed = create_now_playing_embed(track).await;
            print_queue(queue).await;

            embed
        }
        Ordering::Less => {
            tracing::warn!("No tracks in queue, this only happens when an interactive search is done with an empty queue.");
            CreateEmbed::default()
                .description("No tracks in queue!")
                .footer(CreateEmbedFooter::new("No tracks in queue!"))
        }
    };

    edit_embed_response(ctx, embed, msg.clone())
        .await
        .map(|_| ())
}

async fn edit_embed_response(
    ctx: Context<'_>,
    embed: CreateEmbed,
    mut msg: Message,
) -> Result<Message, Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_response(
                &ctx.serenity_context().http,
                EditInteractionResponse::new().add_embed(embed),
            )
            .await
            .map_err(Into::into),
        None => msg
            .edit(
                ctx.serenity_context().http.clone(),
                EditMessage::new().embed(embed),
            )
            .await
            .map(|_| msg)
            .map_err(Into::into),
    }
}

async fn print_queue(queue: Vec<TrackHandle>) {
    for track in queue.iter() {
        let metadata = get_track_metadata(track).await;
        tracing::warn!(
            "Track {}: {} - {}, State: {:?}, Ready: {:?}",
            metadata.title.unwrap_or("".to_string()).red(),
            metadata.artist.unwrap_or("".to_string()).white(),
            metadata.album.unwrap_or("".to_string()).blue(),
            track.get_info().await.unwrap().playing,
            track.get_info().await.unwrap().ready,
        );
    }
}

use std::process::Stdio;
use tokio::process::Command;
async fn download_file_ytdlp(url: &str) -> Result<(Output, AuxMetadata), Error> {
    let metadata = YoutubeDl::new(reqwest::Client::new(), url.to_string())
        .aux_metadata()
        .await?;

    let child = Command::new("yt-dlp")
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    tracing::warn!("yt-dlp");

    let output = child.wait_with_output().await?;
    Ok((output, metadata))
}

async fn yt_search_select(
    ctx: SerenityContext,
    channel_id: ChannelId,
    metadata: Vec<AuxMetadata>,
) -> Result<QueryType, Error> {
    let res = metadata.iter().map(|x| {
        let title = x.title.clone().unwrap_or_default();
        let link = x.source_url.clone().unwrap_or_default();
        // let link = format!("https://www.youtube.com/watch?v={}", link);
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
        }
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
    // // Wait for multiple interactions
    // let mut interaction_stream = m
    //     .await_component_interaction(&ctx.shard)
    //     .timeout(Duration::from_secs(60 * 3))
    //     .stream();

    // while let Some(interaction) = interaction_stream.next().await {
    //     let sound = &interaction.data.custom_id;
    //     // Acknowledge the interaction and send a reply
    //     interaction
    //         .create_response(
    //             &ctx,
    //             // This time we dont edit the message but reply to it
    //             CreateInteractionResponse::Message(
    //                 CreateInteractionResponseMessage::default()
    //                     // Make the message hidden for other users by setting `ephemeral(true)`.
    //                     .ephemeral(true)
    //                     .content(format!("The **{animal}** says __{sound}__")),
    //             ),
    //         )
    //         .await
    //         .unwrap();
    // }

    // // Delete the orig message or there will be dangling components (components that still
    // // exist, but no collector is running so any user who presses them sees an error)
}

async fn create_embed_fields(elems: Vec<AuxMetadata>) -> Vec<EmbedField> {
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

fn duration_to_string(duration: Duration) -> String {
    let mut secs = duration.as_secs();
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    secs %= 60;
    format!("{}:{}:{}", hours, minutes, secs)
}

async fn send_search_response(
    ctx: Context<'_>,
    guild_id: GuildId,
    user_id: UserId,
    query: String,
    res: Vec<AuxMetadata>,
) -> Result<Message, Error> {
    let author = ctx.author_member().await.unwrap();
    let name = if DEFAULT_PREMIUM {
        author.mention().to_string()
    } else {
        author.display_name().to_string()
    };

    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let fields = create_embed_fields(res).await;
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

async fn match_mode(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: QueryType,
) -> Result<bool, Error> {
    // let is_prefix = ctx.prefix() != "/";
    let handler = call.lock().await;
    let queue_was_empty = handler.queue().is_empty();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    drop(handler);

    let pool = ctx.data().database_pool.clone().unwrap();

    tracing::info!("mode: {:?}", mode);

    match mode {
        Mode::Search => {
            // let search_results = match query_type.clone() {
            match query_type.clone() {
                QueryType::Keywords(keywords) => {
                    let reqwest_client = reqwest::Client::new();
                    let search_results = YoutubeDl::new_search(reqwest_client, keywords)
                        .search(None)
                        .await?;
                    // let user_id = ctx.author().id;
                    let qt = yt_search_select(
                        ctx.serenity_context().clone(),
                        ctx.channel_id(),
                        search_results,
                    )
                    .await?;
                    let queue = enqueue_track(ctx.http(), &call, &qt).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await
                    // match_mode(ctx, call.clone(), Mode::End, qt).await
                    // send_search_response(ctx, guild_id, user_id, keywords, search_results).await?;
                }
                QueryType::YoutubeSearch(query) => {
                    let search_results = YoutubeDl::new(reqwest::Client::new(), query.clone())
                        .search(None)
                        .await?;
                    let qt = yt_search_select(
                        ctx.serenity_context().clone(),
                        ctx.channel_id(),
                        search_results,
                    )
                    .await?;
                    // match_mode(ctx, call.clone(), Mode::End, qt).await
                    let queue = enqueue_track(ctx.http(), &call, &qt).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await

                    // let user_id = ctx.author().id;

                    // send_search_response(ctx, guild_id, user_id, query, search_results).await?;
                }
                _ => {
                    let embed = CreateEmbed::default()
                        .description(format!(
                            "{}",
                            CrackedError::Other("Something went wrong while parsing your query!")
                        ))
                        .footer(CreateEmbedFooter::new("Search failed!"));
                    send_embed_response_poise(ctx, embed).await?;
                    return Ok(false);
                }
            };
        }
        Mode::Download => {
            let (status, file_name) = get_download_status_and_filename(query_type.clone()).await?;
            ctx.channel_id()
                .send_message(
                    ctx.http(),
                    CreateMessage::new()
                        .content(format!("Download status {}", status))
                        .add_file(CreateAttachment::path(Path::new(&file_name)).await?),
                )
                .await?;

            return Ok(false);
        }
        Mode::End => match query_type.clone() {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::Jump, QueryType::YoutubeSearch");
                let res = YoutubeDl::new_search(reqwest::Client::new(), query.clone())
                    .search(None)
                    .await?;
                let user_id = ctx.author().id;
                send_search_response(ctx, guild_id, user_id, query.clone(), res).await?;
            }
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::NewYoutubeDl(_) => {
                tracing::warn!("### Mode::End, QueryType::Keywords | QueryType::VideoLink");
                let queue = enqueue_track(ctx.http(), &call, &query_type).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            }
            // FIXME
            QueryType::PlaylistLink(_url) => {
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
                    update_queue_messages(&Arc::new(ctx.http()), ctx.data(), &queue, guild_id)
                        .await;
                }
            }
            QueryType::File(file) => {
                tracing::trace!("Mode::End, QueryType::File");
                let queue = //ffmpeg::from_attachment(file, Metadata::default(), &[]).await?;
                        enqueue_track(ctx.http(), &call, &QueryType::File(file)).await?;
                update_queue_messages(ctx.http(), ctx.data(), &queue, guild_id).await;
            }
        },
        Mode::Next => match query_type.clone() {
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
                tracing::trace!(
                    "Mode::Next, QueryType::Keywords | QueryType::VideoLink | QueryType::File"
                );
                let queue = insert_track(ctx.http(), &call, &query_type, 1).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            }
            // FIXME
            QueryType::PlaylistLink(_url) => {
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
            QueryType::YoutubeSearch(_) => {
                tracing::trace!("Mode::Next, QueryType::YoutubeSearch");
                return Err(CrackedError::Other("Not implemented yet!").into());
            }
        },
        Mode::Jump => match query_type.clone() {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::Jump, QueryType::YoutubeSearch");
                tracing::error!("query: {}", query);
                return Err(CrackedError::Other("Not implemented yet!").into());
            }
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
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
                // FIXME
                let _src = YoutubeDl::new(Client::new(), url);
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
                // FIXME
                let mut src = YoutubeDl::new(reqwest::Client::new(), url);
                let metadata = src.aux_metadata().await?;
                enqueue_track(ctx.http(), &call, &QueryType::NewYoutubeDl((src, metadata))).await?;
                update_queue_messages(
                    &ctx.serenity_context().http,
                    ctx.data(),
                    &call.lock().await.queue().current_queue(),
                    guild_id,
                )
                .await;
            }
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!(
                    "Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::KeywordList"
                );
                for keywords in keywords_list.into_iter() {
                    let queue = enqueue_track(
                        &ctx.serenity_context().http,
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
                edit_response_poise(ctx, CrackedMessage::PlayAllFailed).await?;
                return Ok(false);
            }
        },
    }

    Ok(true)
}

use colored::Colorize;
/// Matches a url (or query string) to a QueryType
async fn get_query_type_from_url(
    ctx: Context<'_>,
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
                tracing::warn!("spotify: {} -> {}", url, final_url);
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, &final_url).await?)
            }
            Some("cdn.discordapp.com") => {
                tracing::warn!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            }

            Some(other) => {
                let mut settings = ctx.data().guild_settings_map.write().unwrap().clone();
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

                        send_response_poise_text(ctx, message).await?;
                    }
                }

                //YouTube::extract(url)
                let mut yt = YoutubeDl::new(reqwest::Client::new(), url.to_string());
                let metadata = yt.aux_metadata().await?;
                Some(QueryType::NewYoutubeDl((yt, metadata)))
            }
            None => {
                // handle spotify:track:3Vr5jdQHibI2q0A0KW4RWk format?
                // TODO: Why is this a thing?
                if url.starts_with("spotify:") {
                    let parts = url.split(':').collect::<Vec<_>>();
                    let final_url =
                        format!("https://open.spotify.com/track/{}", parts.last().unwrap());
                    tracing::warn!("spotify: {} -> {}", url, final_url);
                    let spotify = SPOTIFY.lock().await;
                    let spotify =
                        verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                    Some(Spotify::extract(spotify, &final_url).await?)
                } else {
                    Some(QueryType::Keywords(url.to_string()))
                    //                None
                }
            }
        },
        Err(e) => {
            tracing::error!("Url::parse error: {}", e);
            Some(QueryType::Keywords(url.to_string()))
            // if url.contains("ytsearch") {
            //     let search_query = QueryType::YoutubeSearch(url.to_string());
            //     tracing::error!("search_query: {:?}", search_query);
            //     Some(search_query)
            // } else {
            // let settings = ctx.data().guild_settings_map.write().unwrap().clone();
            // let guild_settings = settings.get(&guild_id).unwrap();
            // if !guild_settings.allow_all_domains.unwrap_or(true)
            //     && (guild_settings.banned_domains.contains("youtube.com")
            //         || (guild_settings.banned_domains.is_empty()
            //             && !guild_settings.allowed_domains.contains("youtube.com")))
            // {
            //     let message = CrackedMessage::PlayDomainBanned {
            //         domain: "youtube.com".to_string(),
            //     };

            //     send_response_poise_text(ctx, message).await?;
            // }

            // Some(QueryType::Keywords(url.to_string()))
            // None
            // }
        }
    };

    let res = if let Some(QueryType::Keywords(_)) = query_type {
        let settings = ctx.data().guild_settings_map.write().unwrap().clone();
        let guild_settings = settings.get(&guild_id).unwrap();
        if !guild_settings.allow_all_domains.unwrap_or(true)
            && (guild_settings.banned_domains.contains("youtube.com")
                || (guild_settings.banned_domains.is_empty()
                    && !guild_settings.allowed_domains.contains("youtube.com")))
        {
            let message = CrackedMessage::PlayDomainBanned {
                domain: "youtube.com".to_string(),
            };

            send_response_poise_text(ctx, message).await?;
            Ok(None)
        } else {
            Result::Ok(query_type)
        }
    } else {
        Result::Ok(query_type)
    };
    res
}

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
        }
    }
}

#[derive(Debug, Clone)]
pub enum MyAuxMetadata {
    Data(AuxMetadata),
}

impl TypeMapKey for MyAuxMetadata {
    type Value = MyAuxMetadata;
}

impl Default for MyAuxMetadata {
    fn default() -> Self {
        MyAuxMetadata::Data(AuxMetadata::default())
    }
}

async fn build_queued_embed(
    title: &str,
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

    let title_text = &format!("[**{}**]({})", meta_title, source_url);

    let footer_text = format!(
        "{}{}\n{}{}",
        TRACK_DURATION,
        get_human_readable_timestamp(metadata.duration),
        TRACK_TIME_TO_PLAY,
        get_human_readable_timestamp(Some(estimated_time))
    );

    CreateEmbed::new()
        .title(title)
        .thumbnail(thumbnail)
        .field(title_text, "", false)
        .footer(CreateEmbedFooter::new(footer_text))
}

// FIXME: Do you want to have a reqwest client we keep around and pass into
// this instead of creating a new one every time?
// FIXME: This is super expensive, literally we need to do this a lot better.
async fn get_download_status_and_filename(query_type: QueryType) -> Result<(bool, String), Error> {
    let client = reqwest::Client::new();
    tracing::warn!("query_type: {:?}", query_type);
    match query_type {
        QueryType::YoutubeSearch(_) => Err(Box::new(CrackedError::Other(
            "Download not valid with search results.",
        ))),
        QueryType::VideoLink(url) => {
            tracing::warn!("Mode::Download, QueryType::VideoLink");
            let (output, metadata) = download_file_ytdlp(&url).await?;
            let status = output.status.success();
            let url = metadata.source_url.unwrap();
            let file_name = format!(
                "/home/lothrop/src/cracktunes/{} [{}].webm",
                metadata.title.unwrap(),
                url.split('=').last().unwrap()
            );
            Ok((status, file_name))
        }
        QueryType::NewYoutubeDl((_src, metadata)) => {
            tracing::warn!("Mode::Download, QueryType::NewYoutubeDl");
            let url = metadata.source_url.unwrap();
            let file_name = format!(
                "/home/lothrop/src/cracktunes/{} [{}].webm",
                metadata.title.unwrap(),
                url.split('=').last().unwrap()
            );
            tracing::warn!("file_name: {}", file_name);
            let (output, _metadata) = download_file_ytdlp(&url).await?;
            let status = output.status.success();
            Ok((status, file_name))
        }
        QueryType::Keywords(query) => {
            tracing::warn!("In Keywords");
            let mut ytdl = YoutubeDl::new(client, format!("ytsearch:{}", query));
            let metadata = ytdl.aux_metadata().await.unwrap();
            let url = metadata.source_url.unwrap();
            let (output, metadata) = download_file_ytdlp(&url).await?;

            let file_name = format!(
                "/home/lothrop/src/cracktunes/{} [{}].webm",
                metadata.title.unwrap(),
                url.split('=').last().unwrap()
            );
            let status = output.status.success();
            Ok((status, file_name))
        }
        QueryType::File(file) => {
            tracing::warn!("In File");
            Ok((true, file.url.to_owned().to_string()))
        }
        QueryType::PlaylistLink(url) => {
            tracing::warn!("In PlaylistLink");
            let (output, metadata) = download_file_ytdlp(&url).await?;
            let file_name = format!(
                "/home/lothrop/src/cracktunes/{} [{}].webm",
                metadata.title.unwrap(),
                url.split('=').last().unwrap()
            );
            let status = output.status.success();
            Ok((status, file_name))
        }
        QueryType::KeywordList(keywords_list) => {
            tracing::warn!("In KeywordList");
            let url = format!("ytsearch:{}", keywords_list.join(" "));
            let mut ytdl = YoutubeDl::new(client, url.clone());
            tracing::warn!("ytdl: {:?}", ytdl);
            let metadata = ytdl.aux_metadata().await.unwrap();
            let (output, _metadata) = download_file_ytdlp(&url).await?;
            let file_name = format!(
                "/home/lothrop/src/cracktunes/{} [{}].webm",
                metadata.title.unwrap(),
                url.split('=').last().unwrap()
            );
            let status = output.status.success();
            Ok((status, file_name))
        }
    }
}

// FIXME: Do you want to have a reqwest client we keep around and pass into
// this instead of creating a new one every time?
async fn get_track_source_and_metadata(
    _http: &Http,
    query_type: QueryType,
) -> (SongbirdInput, Vec<MyAuxMetadata>) {
    let client = reqwest::Client::new();
    tracing::warn!("query_type: {:?}", query_type);
    match query_type {
        QueryType::YoutubeSearch(query) => {
            tracing::error!("In YoutubeSearch");
            let mut ytdl = YoutubeDl::new_search(client, query);
            let mut res = Vec::new();
            let asdf = ytdl.search(None).await.unwrap_or_default();
            for metadata in asdf {
                let my_metadata = MyAuxMetadata::Data(metadata);
                res.push(my_metadata);
            }
            (ytdl.into(), res)
        }
        QueryType::VideoLink(query) => {
            tracing::warn!("In VideoLink");
            let mut ytdl = YoutubeDl::new(client, query);
            tracing::warn!("ytdl: {:?}", ytdl);
            let metadata = ytdl.aux_metadata().await.unwrap_or_default();
            let my_metadata = MyAuxMetadata::Data(metadata);
            (ytdl.into(), vec![my_metadata])
        }
        QueryType::Keywords(query) => {
            tracing::warn!("In Keywords");
            let mut ytdl = YoutubeDl::new(client, format!("ytsearch:{}", query));
            let metdata = ytdl.aux_metadata().await.unwrap_or_default();
            let my_metadata = MyAuxMetadata::Data(metdata);
            (ytdl.into(), vec![my_metadata])
        }
        QueryType::File(file) => {
            tracing::warn!("In File");
            (
                HttpRequest::new(client, file.url.to_owned()).into(),
                vec![MyAuxMetadata::default()],
            )
        }
        QueryType::NewYoutubeDl(ytdl) => {
            tracing::warn!("In NewYoutubeDl {:?}", ytdl.0);
            (ytdl.0.into(), vec![MyAuxMetadata::Data(ytdl.1)])
        }
        QueryType::PlaylistLink(url) => {
            tracing::warn!("In PlaylistLink");
            let mut ytdl = YoutubeDl::new(client, url);
            tracing::warn!("ytdl: {:?}", ytdl);
            let metdata = ytdl.aux_metadata().await.unwrap();
            let my_metadata = MyAuxMetadata::Data(metdata);
            (ytdl.into(), vec![my_metadata])
        }
        QueryType::KeywordList(keywords_list) => {
            tracing::warn!("In KeywordList");
            let mut ytdl = YoutubeDl::new(client, format!("ytsearch:{}", keywords_list.join(" ")));
            tracing::warn!("ytdl: {:?}", ytdl);
            let metdata = ytdl.aux_metadata().await.unwrap();
            let my_metadata = MyAuxMetadata::Data(metdata);
            (ytdl.into(), vec![my_metadata])
        }
    }
}

async fn enqueue_track_pgwrite(
    database_pool: &PgPool,
    guild_id: GuildId,
    channel_id: ChannelId,
    http: &Http,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    tracing::info!("query_type: {:?}", query_type);
    // is this comment still relevant to this section of code?
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(http, query_type.clone()).await;
    let res = metadata.first().unwrap().clone();
    let track: Track = source.into();

    let MyAuxMetadata::Data(res2) = res.clone();
    let (asdf, _qwer) =
        aux_metadata_to_db_structures(&res2, guild_id.get() as i64, channel_id.get() as i64)?;
    crate::db::metadata::Metadata::create(database_pool, asdf).await?;

    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(res.clone());

    Ok(handler.queue().current_queue())
}

async fn enqueue_track(
    http: &Http,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    tracing::info!("query_type: {:?}", query_type);
    // is this comment still relevant to this section of code?
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(http, query_type.clone()).await;
    let res = metadata.first().unwrap().clone();
    let track: Track = source.into();

    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(res.clone());

    Ok(handler.queue().current_queue())
}

async fn insert_track(
    http: &Http,
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

/// Rotates the queue by `n` tracks to the right.
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_mode() {
        let is_prefix = true;
        let x = "asdf".to_string();
        let msg = Some(x.clone());
        let mode = Some("".to_string());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let x = "".to_string();
        let is_prefix = true;
        let msg = None;
        let mode = Some(x.clone());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let is_prefix = true;
        let msg = None;
        let mode = None;

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let is_prefix = false;
        let msg = Some(x.clone());
        let mode = Some("next".to_string());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::Next, x.clone()));

        let is_prefix = false;
        let msg = None;
        let mode = Some("download".to_string());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::Download, x.clone()));

        let is_prefix = false;
        let msg = None;
        let mode = None;

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x));
    }

    #[test]
    fn test_get_msg() {
        let mode = Some("".to_string());
        let query_or_url = Some("".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = None;
        let query_or_url = Some("".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = None;
        let query_or_url = None;
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = Some("".to_string());
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("".to_string()));

        let mode = None;
        let query_or_url = Some("".to_string());
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("".to_string()));

        let mode = None;
        let query_or_url = None;
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = None;
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = None;
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode: Option<String> = None;
        let query_or_url = Some("asdf asdf asdf asd f".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("asdf asdf asdf asd f".to_string()));
    }
}
