use rusty_ytdl::search::{SearchOptions, SearchType};
use songbird::input::{AuxMetadata, Compose, HttpRequest, Input as SongbirdInput, YoutubeDl};

use crate::{
    commands::MyAuxMetadata, errors::CrackedError, http_utils,
    sources::rusty_ytdl::RustyYoutubeClient,
};

use super::QueryType;

/// Get the source and metadata from a video link.
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
            tracing::warn!("In KeywordList");
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
