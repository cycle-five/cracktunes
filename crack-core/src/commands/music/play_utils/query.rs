use crate::{
    commands::{check_banned_domains, MyAuxMetadata},
    errors::{verify, CrackedError},
    guild::settings::GuildSettings,
    http_utils,
    messaging::{message::CrackedMessage, messages::SPOTIFY_AUTH_FAILED},
    sources::{
        rusty_ytdl::RustyYoutubeClient,
        spotify::{Spotify, SpotifyTrack, SPOTIFY},
        youtube::{
            search_query_to_source_and_metadata_rusty, search_query_to_source_and_metadata_ytdl,
            video_info_to_source_and_metadata,
        },
    },
    utils::{compare_domains, get_guild_name, send_response_poise_text},
    Context, Error,
};
use ::serenity::all::Attachment;
use colored::Colorize;
use poise::serenity_prelude as serenity;
use rusty_ytdl::search::{SearchOptions, SearchType};
use songbird::input::{AuxMetadata, Compose as _, HttpRequest, Input as SongbirdInput, YoutubeDl};
use url::Url;

/// Enum for type of possible queries we have to handle
#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(serenity::Attachment),
    NewYoutubeDl((YoutubeDl, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

impl QueryType {
    /// Build a query string from the query type.
    pub fn build_query(&self) -> Option<String> {
        match self {
            QueryType::Keywords(keywords) => Some(keywords.clone()),
            QueryType::KeywordList(keywords_list) => Some(keywords_list.join(" ")),
            QueryType::VideoLink(url) => Some(url.clone()),
            QueryType::SpotifyTracks(tracks) => Some(
                tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>()
                    .join(" "),
            ),
            QueryType::PlaylistLink(url) => Some(url.clone()),
            QueryType::File(file) => Some(file.url.clone()),
            QueryType::NewYoutubeDl((_src, metadata)) => metadata.source_url.clone(),
            QueryType::YoutubeSearch(query) => Some(query.clone()),
            QueryType::None => None,
        }
    }

    // FIXME: Do you want to have a reqwest client we keep around and pass into
    // this instead of creating a new one every time?
    pub async fn get_track_source_and_metadata(
        &self,
    ) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
        use colored::Colorize;
        let client = http_utils::get_client().clone();
        tracing::warn!("{}", format!("query_type: {:?}", self).red());
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let mut ytdl = YoutubeDl::new_search(client, query.clone());
                let mut res = Vec::new();
                let asdf = ytdl.search(None).await?;
                for metadata in asdf {
                    let my_metadata = MyAuxMetadata::Data(metadata);
                    res.push(my_metadata);
                }
                Ok((ytdl.into(), res))
            },
            QueryType::VideoLink(query) => {
                tracing::warn!("In VideoLink");
                video_info_to_source_and_metadata(client.clone(), query.clone()).await
                // let mut ytdl = YoutubeDl::new(client, query);
                // tracing::warn!("ytdl: {:?}", ytdl);
                // let metadata = ytdl.aux_metadata().await?;
                // let my_metadata = MyAuxMetadata::Data(metadata);
                // Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::Keywords(query) => {
                tracing::warn!("In Keywords");
                let res = search_query_to_source_and_metadata_rusty(
                    client.clone(),
                    QueryType::Keywords(query.clone()),
                )
                .await;
                match res {
                    Ok((input, metadata)) => Ok((input, metadata)),
                    Err(_) => {
                        tracing::error!("falling back to ytdl!");
                        search_query_to_source_and_metadata_ytdl(client.clone(), query.clone())
                            .await
                    },
                }
            },
            QueryType::File(file) => {
                tracing::warn!("In File");
                Ok((
                    HttpRequest::new(client, file.url.to_owned()).into(),
                    vec![MyAuxMetadata::default()],
                ))
            },
            QueryType::NewYoutubeDl(ytdl_metadata) => {
                tracing::warn!("In NewYoutubeDl {:?}", ytdl_metadata.clone());
                let (ytdl, aux_metadata) = ytdl_metadata.clone();
                Ok((ytdl.into(), vec![MyAuxMetadata::Data(aux_metadata)]))
            },
            QueryType::PlaylistLink(url) => {
                tracing::warn!("In PlaylistLink");
                let rytdl = RustyYoutubeClient::new_with_client(client.clone()).unwrap();
                let search_options = SearchOptions {
                    limit: 100,
                    search_type: SearchType::Playlist,
                    ..Default::default()
                };

                let res = rytdl.rusty_ytdl.search(url, Some(&search_options)).await?;
                let mut metadata = Vec::with_capacity(res.len());
                for r in res {
                    metadata.push(MyAuxMetadata::Data(
                        RustyYoutubeClient::search_result_to_aux_metadata(&r),
                    ));
                }
                let ytdl = YoutubeDl::new(client.clone(), url.clone());
                tracing::warn!("ytdl: {:?}", ytdl);
                Ok((ytdl.into(), metadata))
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::error!("In SpotifyTracks, this is broken");
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                let mut ytdl = YoutubeDl::new(
                    client,
                    format!("ytsearch:{}", keywords_list.first().unwrap()),
                );
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = MyAuxMetadata::Data(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::warn!("In KeywordList");
                let mut ytdl =
                    YoutubeDl::new(client, format!("ytsearch:{}", keywords_list.join(" ")));
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = MyAuxMetadata::Data(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::None => unimplemented!(),
        }
    }
}

/// Returns the QueryType for a given URL (or query string, or file attachment)
pub async fn query_type_from_url(
    ctx: Context<'_>,
    url: &str,
    file: Option<Attachment>,
) -> Result<Option<QueryType>, Error> {
    tracing::info!("url: {}", url);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") | Some("spotify.link") => {
                let final_url = http_utils::resolve_final_url(url).await?;
                tracing::info!(
                    "spotify: {} -> {}",
                    url.underline().blue(),
                    final_url.underline().bright_blue()
                );
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, &final_url).await?)
            },
            Some("cdn.discordapp.com") => {
                tracing::info!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            },

            Some(other) => {
                let data = ctx.data();
                let mut settings = data.guild_settings_map.write().unwrap().clone();
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

                // Handle youtube playlist
                if url.contains("list=") {
                    tracing::warn!("{}: {}", "youtube playlist".blue(), url.underline().blue());
                    Some(QueryType::PlaylistLink(url.to_string()))
                } else {
                    tracing::warn!("{}: {}", "youtube video".blue(), url.underline().blue());
                    let rusty_ytdl = RustyYoutubeClient::new()?;
                    let res_info = rusty_ytdl.get_video_info(url.to_string()).await;
                    let metadata = match res_info {
                        Ok(info) => RustyYoutubeClient::video_info_to_aux_metadata(&info),
                        _ => {
                            tracing::warn!("info: None, falling back to yt-dlp");
                            AuxMetadata {
                                source_url: Some(url.to_string()),
                                ..AuxMetadata::default()
                            }
                        },
                    };
                    let yt = YoutubeDl::new(http_utils::get_client().clone(), url.to_string());
                    Some(QueryType::NewYoutubeDl((yt, metadata)))
                }
            },
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
            },
        },
        Err(e) => {
            tracing::error!("Url::parse error: {}", e);
            Some(QueryType::Keywords(url.to_string()))
        },
    };
    let guild_settings = ctx
        .data()
        .get_guild_settings(guild_id)
        .ok_or(CrackedError::NoGuildSettings)?;
    check_banned_domains(&guild_settings, query_type).map_err(Into::into)
}

// #[derive(Clone, Debug)]
// pub struct Query {
//     pub query_type: QueryType,
//     pub metadata: Option<AuxMetadata>,
// }

// impl Query {
//     pub fn build_query(&self) -> Option<String> {
//         self.query_type.build_query()
//     }

//     pub async fn query(&self, n: usize) -> Result<(), CrackedError> {
//         let _ = n;
//         match self.query_type {
//             QueryType::Keywords(_) => Ok(()),
//             QueryType::KeywordList(_) => Ok(()),
//             QueryType::VideoLink(_) => Ok(()),
//             QueryType::SpotifyTracks(_) => Ok(()),
//             QueryType::PlaylistLink(_) => Ok(()),
//             QueryType::File(_) => Ok(()),
//             QueryType::NewYoutubeDl(_) => Ok(()),
//             QueryType::YoutubeSearch(_) => Ok(()),
//             QueryType::None => Err(CrackedError::Other("No query provided!")),
//         }
//     }

//     pub fn metadata(&self) -> Option<AuxMetadata> {
//         match &self.query_type {
//             QueryType::NewYoutubeDl((_src, metadata)) => Some(metadata.clone()),
//             _ => None,
//         }
//     }

//     pub async fn aux_metadata(&mut self) -> Result<AuxMetadata, CrackedError> {
//         if let Some(meta) = self.metadata.as_ref() {
//             return Ok(meta.clone());
//         }

//         self.query(1).await?;

//         self.metadata.clone().ok_or_else(|| {
//             CrackedError::Other("Failed to instansiate any metadata... Should be unreachable.")
//             // let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
//             //     "Failed to instansiate any metadata... Should be unreachable.".into();
//             // CrackedError::AudioStream(AudioStreamError::Fail(msg))
//         })
//     }
// }
