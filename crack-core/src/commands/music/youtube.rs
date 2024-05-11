use super::{QueryType, RequestingUser};
use crate::http_utils::CacheHttpExt;
use crate::Context as CrackContext;
use crate::{
    commands::MyAuxMetadata, errors::CrackedError, http_utils,
    sources::rusty_ytdl::RustyYoutubeClient,
};
use rusty_ytdl::search::{SearchOptions, SearchType};
use serenity::all::UserId;
use songbird::{
    input::{AuxMetadata, Compose, HttpRequest, Input as SongbirdInput, YoutubeDl},
    tracks::{Track, TrackHandle},
    Call,
};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TrackReadyData {
    pub track: Track,
    pub metadata: MyAuxMetadata,
    pub username: String,
    pub user_id: UserId,
}

/// Takes a query and returns a track that is ready to be played, along with relevant metadata.
pub async fn ready_query(
    ctx: CrackContext<'_>,
    query_type: QueryType,
) -> Result<TrackReadyData, CrackedError> {
    let user_id = ctx.author().id;
    let (source, metadata_vec): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(query_type.clone()).await?;
    let metadata = match metadata_vec.first() {
        Some(x) => x.clone(),
        None => {
            return Err(CrackedError::Other("metadata.first() failed"));
        },
    };
    let track: Track = source.into();

    let username = ctx.user_id_to_username_or_default(user_id);

    Ok(TrackReadyData {
        track,
        metadata,
        username,
        user_id,
    })
}

/// Pushes a track to the front of the queue, after readying it.
pub async fn push_front_track_ready(
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(ready_track.track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(ready_track.metadata.clone());
    map.insert::<RequestingUser>(RequestingUser::UserId(ready_track.user_id));
    handler.queue().modify_queue(|queue| {
        let back = queue.pop_back().unwrap();
        queue.push_front(back);
    });

    Ok(handler.queue().current_queue())
}

/// Pushes a track to the back of the queue, after readying it.
pub async fn enqueue_track_ready(
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(ready_track.track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(ready_track.metadata.clone());
    map.insert::<RequestingUser>(RequestingUser::UserId(ready_track.user_id));
    Ok(handler.queue().current_queue())
}

/// Pushes a track to the front of the queue.
pub async fn queue_track_front(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    push_front_track_ready(call, ready_track).await
}

/// Pushes a track to the front of the queue.
pub async fn queue_track_back(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    enqueue_track_ready(call, ready_track).await
}

/// Get the source and metadata from a video link. Return value is a vector due
/// to this being used in a method that also handles the interactive search so
/// it can return multiple metadatas.
pub async fn video_info_to_source_and_metadata(
    client: reqwest::Client,
    url: String,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    let rytdl = RustyYoutubeClient::new_with_client(client.clone())?;
    let video_info = rytdl.get_video_info(url.clone()).await?;
    let metadata = RustyYoutubeClient::video_info_to_aux_metadata(&video_info);
    let my_metadata = MyAuxMetadata::Data(metadata);

    let ytdl = YoutubeDl::new(client, url);
    Ok((ytdl.into(), vec![my_metadata]))
}

/// Search youtube for a query and return the source (playable)
/// and metadata.
pub async fn search_query_to_source_and_metadata(
    client: reqwest::Client,
    query: String,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    tracing::warn!("search_query_to_source_and_metadata: {:?}", query);
    let metadata = {
        let rytdl = RustyYoutubeClient::new_with_client(client.clone())?;
        // let rytdl = RustyYoutubeClient::new()?;
        tracing::warn!("search_query_to_source_and_metadata: {:?}", rytdl);
        let results = rytdl.one_shot(query.clone()).await?;
        tracing::warn!("search_query_to_source_and_metadata: {:?}", results);
        // FIXME: Fallback to yt-dlp
        let result = match results {
            Some(r) => r,
            None => return Err(CrackedError::EmptySearchResult),
        };
        let metadata = &RustyYoutubeClient::search_result_to_aux_metadata(&result);
        metadata.clone()
    };
    let source_url = match metadata.clone().source_url {
        Some(url) => url.clone(),
        None => "".to_string(),
    };
    let ytdl = YoutubeDl::new(client, source_url);
    let my_metadata = MyAuxMetadata::Data(metadata);

    Ok((ytdl.into(), vec![my_metadata]))
}

/// Search youtube for a query and return the source (playable)
/// and metadata using the yt-dlp command line tool.
pub async fn search_query_to_source_and_metadata_ytdl(
    client: reqwest::Client,
    query: String,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    let query = if query.starts_with("ytsearch:") {
        query
    } else {
        format!("ytsearch:{}", query)
    };
    let mut ytdl = YoutubeDl::new(client, query);
    let metadata = ytdl.aux_metadata().await?;
    let my_metadata = MyAuxMetadata::Data(metadata);

    Ok((ytdl.into(), vec![my_metadata]))
}

// FIXME: Do you want to have a reqwest client we keep around and pass into
// this instead of creating a new one every time?
pub async fn get_track_source_and_metadata(
    query_type: QueryType,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    use colored::Colorize;
    let client = http_utils::get_client().clone();
    tracing::warn!("{}", format!("query_type: {:?}", query_type).red());
    match query_type {
        QueryType::YoutubeSearch(query) => {
            tracing::error!("In YoutubeSearch");
            let mut ytdl = YoutubeDl::new_search(client, query);
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
            video_info_to_source_and_metadata(client.clone(), query).await
            // let mut ytdl = YoutubeDl::new(client, query);
            // tracing::warn!("ytdl: {:?}", ytdl);
            // let metadata = ytdl.aux_metadata().await?;
            // let my_metadata = MyAuxMetadata::Data(metadata);
            // Ok((ytdl.into(), vec![my_metadata]))
        },
        QueryType::Keywords(query) => {
            tracing::warn!("In Keywords");
            let res = search_query_to_source_and_metadata(client.clone(), query.clone()).await;
            match res {
                Ok((input, metadata)) => Ok((input, metadata)),
                Err(_) => {
                    tracing::error!("falling back to ytdl!");
                    search_query_to_source_and_metadata_ytdl(client.clone(), query).await
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
        QueryType::NewYoutubeDl(ytdl) => {
            tracing::warn!("In NewYoutubeDl {:?}", ytdl.0);
            Ok((ytdl.0.into(), vec![MyAuxMetadata::Data(ytdl.1)]))
        },
        QueryType::PlaylistLink(url) => {
            tracing::warn!("In PlaylistLink");
            let rytdl = RustyYoutubeClient::new_with_client(client.clone()).unwrap();
            let search_options = SearchOptions {
                limit: 100,
                search_type: SearchType::Playlist,
                ..Default::default()
            };

            let res = rytdl
                .rusty_ytdl
                .search(&url, Some(&search_options))
                .await
                .unwrap();
            let mut metadata = Vec::with_capacity(res.len());
            for r in res {
                metadata.push(MyAuxMetadata::Data(
                    RustyYoutubeClient::search_result_to_aux_metadata(&r),
                ));
            }
            let ytdl = YoutubeDl::new(client.clone(), url);
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
            let mut ytdl = YoutubeDl::new(client, format!("ytsearch:{}", keywords_list.join(" ")));
            tracing::warn!("ytdl: {:?}", ytdl);
            let metdata = ytdl.aux_metadata().await.unwrap();
            let my_metadata = MyAuxMetadata::Data(metdata);
            Ok((ytdl.into(), vec![my_metadata]))
        },
        QueryType::None => unimplemented!(),
    }
}

/// Build a query from AuxMetadata.
pub fn build_query_aux_metadata(aux_metadata: &AuxMetadata) -> String {
    format!(
        "{} - {}",
        aux_metadata.artist.clone().unwrap_or_default(),
        aux_metadata.track.clone().unwrap_or_default(),
    )
}

#[cfg(test)]
mod test {

    use super::*;

    #[tokio::test]
    async fn test_get_track_source_and_metadata() {
        let query_type = QueryType::Keywords("hello".to_string());
        let res = get_track_source_and_metadata(query_type).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_ytdl() {
        let query_type = QueryType::Keywords("hello".to_string());
        let res = get_track_source_and_metadata(query_type).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_video_link() {
        let query_type =
            QueryType::VideoLink("https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string());
        let res = get_track_source_and_metadata(query_type).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_playlist_link() {
        let query_type = QueryType::PlaylistLink(
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".to_string(),
        );
        let res = get_track_source_and_metadata(query_type).await;
        assert!(res.is_ok());
    }

    // #[tokio::test]
    // async fn test_get_track_source_and_metadata_spotify_tracks() {
    //     let query_type = QueryType::SpotifyTracks(vec![SpotifyTrack {
    //         full_track: FullTrack {
    //         },
    //     }]);
    //     let res = get_track_source_and_metadata(query_type).await;
    //     assert!(res.is_ok());
    // }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_keyword_list() {
        let query_type = QueryType::KeywordList(vec!["hello".to_string(), "world".to_string()]);
        let res = get_track_source_and_metadata(query_type).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_build_query_aux_metadata() {
        let aux_metadata = AuxMetadata {
            artist: Some("hello".to_string()),
            track: Some("world".to_string()),
            ..Default::default()
        };
        let res = build_query_aux_metadata(&aux_metadata);
        assert_eq!(res, "hello - world");
    }

    #[tokio::test]
    async fn test_video_info_to_source_and_metadata() {
        let client = reqwest::Client::new();
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let res = video_info_to_source_and_metadata(client, url).await;
        assert!(res.is_ok());
    }
}
