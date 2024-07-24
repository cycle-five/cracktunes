use std::sync::Arc;

use crate::commands::play_utils::QueryType;
use crate::sources::rusty_ytdl::RustyYoutubeSearch;
use crate::CrackedResult;
use crate::{
    commands::MyAuxMetadata, errors::CrackedError, sources::rusty_ytdl::RustyYoutubeClient,
};
use rusty_ytdl::{RequestOptions, Video, VideoOptions};
use songbird::input::{AuxMetadata, Compose, Input as SongbirdInput, YoutubeDl};

/// Get the source and metadata from a video link. Return value is a vector due
/// to this being used in a method that also handles the interactive search so
/// it can return multiple metadatas.
pub async fn get_rusty_search(
    client: reqwest::Client,
    url: String,
) -> CrackedResult<RustyYoutubeSearch> {
    let video_options = VideoOptions {
        request_options: RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    let video = Video::new_with_options(url.clone(), video_options)?;
    let video_info = video.get_info().await?;
    let rytdl = RustyYoutubeClient::new_with_client(client.clone())?;
    let metadata = RustyYoutubeClient::video_info_to_aux_metadata(&video_info);

    let rusty_search = RustyYoutubeSearch {
        rusty_ytdl: rytdl,
        metadata: Some(metadata.clone()),
        query: QueryType::VideoLink(url.clone()),
        url: Some(url),
        video: Some(Arc::new(video)),
    };
    Ok(rusty_search)
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

        let results = rytdl.search_one(query.clone()).await?;

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
    let my_metadata = MyAuxMetadata(metadata);

    Ok((ytdl.into(), vec![my_metadata]))
}

/// Search youtube for a query and return the source (playable)
/// and metadata.
pub async fn search_query_to_source_and_metadata_rusty(
    client: reqwest::Client,
    query: QueryType,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    tracing::warn!("search_query_to_source_and_metadata_rusty: {:?}", query);
    let rytdl = RustyYoutubeClient::new_with_client(client.clone())?;

    let metadata = {
        // let rytdl = RustyYoutubeClient::new()?;
        tracing::warn!("search_query_to_source_and_metadata_rusty: {:?}", rytdl);
        let results = rytdl
            .search_one(
                query
                    .build_query()
                    .ok_or(CrackedError::Other("No query given"))?,
            )
            .await?;
        tracing::warn!("search_query_to_source_and_metadata_rusty: {:?}", results);
        // FIXME: Fallback to yt-dlp
        let result = match results {
            Some(r) => r,
            None => return Err(CrackedError::EmptySearchResult),
        };
        let metadata = &RustyYoutubeClient::search_result_to_aux_metadata(&result);
        metadata.clone()
    };

    let rusty_search = RustyYoutubeSearch {
        rusty_ytdl: rytdl,
        metadata: Some(metadata.clone()),
        query,
        url: metadata.source_url.clone(),
        video: None,
    };

    Ok((rusty_search.into(), vec![MyAuxMetadata(metadata)]))
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
    let my_metadata = MyAuxMetadata(metadata);

    Ok((ytdl.into(), vec![my_metadata]))
}

/// Build a query from AuxMetadata.
pub fn build_query_aux_metadata(aux_metadata: &AuxMetadata) -> String {
    format!(
        "{} - {}",
        aux_metadata.track.clone().unwrap_or_default(),
        aux_metadata.artist.clone().unwrap_or_default(),
    )
}

#[cfg(test)]
mod test {

    use rusty_ytdl::search::YouTube;

    use crate::http_utils;

    use super::*;

    #[tokio::test]
    async fn test_get_track_metadata_video_link() {
        let opts = RequestOptions {
            client: Some(http_utils::get_client().clone()),
            ..Default::default()
        };
        let reqclient = http_utils::get_client().clone();
        let ytclient = YouTube::new_with_options(&opts).unwrap();
        let query_type =
            QueryType::VideoLink("https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string());
        let res = query_type.get_track_metadata(ytclient, reqclient).await;
        if let Err(ref e) = res {
            tracing::warn!("Error: {:?}", e);
        }
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata() {
        let query_type = QueryType::Keywords("hello".to_string());
        let res = query_type.get_track_source_and_metadata().await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_ytdl() {
        let query_type = QueryType::Keywords("hello".to_string());
        let res = query_type.get_track_source_and_metadata().await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_video_link() {
        let query_type =
            QueryType::VideoLink("https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string());
        let res = query_type.get_track_source_and_metadata().await;
        if let Err(ref e) = res {
            tracing::warn!("Error: {:?}", e);
        }
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_playlist_link() {
        let query_type = QueryType::PlaylistLink(
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".to_string(),
        );
        let res = query_type.get_track_source_and_metadata().await;
        if let Err(ref e) = res {
            tracing::warn!("Error: {:?}", e);
        }
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_keyword_list() {
        let query_type = QueryType::KeywordList(vec!["hello".to_string(), "world".to_string()]);
        let res = query_type.get_track_source_and_metadata().await;
        match res {
            Ok(_) => assert!(true),
            Err(e) => {
                let phrase = "Your IP is likely being blocked by Youtube";
                assert!(e.to_string().contains(phrase));
            },
        }
    }

    #[tokio::test]
    async fn test_build_query_aux_metadata() {
        let aux_metadata = AuxMetadata {
            artist: Some("hello".to_string()),
            track: Some("world".to_string()),
            ..Default::default()
        };
        let res = build_query_aux_metadata(&aux_metadata);
        assert_eq!(res, "world - hello");
    }

    #[tokio::test]
    async fn test_get_rusty_search() {
        let client = reqwest::Client::new();
        let url = "https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string();
        let res = get_rusty_search(client, url).await;

        match res {
            Ok(search) => assert!(search.metadata.is_some()),
            Err(e) => {
                let phrase = "Your IP is likely being blocked by Youtube";
                assert!(e.to_string().contains(phrase));
            },
        }
    }
}
