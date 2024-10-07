use crate::errors::CrackedError;
use crate::http_utils;
use crate::music::query::QueryType;
use crate::sources::rusty_ytdl::{
    search_result_to_aux_metadata, video_info_to_aux_metadata, RustyYoutubeSearch,
};
use crate::utils::MUSIC_SEARCH_SUFFIX;
use crate::CrackedResult;
use crack_types::MyAuxMetadata;
use rusty_ytdl::{RequestOptions, Video, VideoOptions};
use songbird::input::{AuxMetadata, Compose, Input as SongbirdInput, YoutubeDl};
use urlencoding::encode;

/// Get the source and metadata from a video link. Return value is a vector due
/// to this being used in a method that also handles the interactive search so
/// it can return multiple metadatas.
pub async fn get_rusty_search(
    client: reqwest::Client,
    url: String,
) -> CrackedResult<RustyYoutubeSearch> {
    let request_options = RequestOptions {
        client: Some(client.clone()),
        ..Default::default()
    };
    let video_options = VideoOptions {
        request_options: request_options.clone(),
        ..Default::default()
    };
    let video = Video::new_with_options(url.clone(), video_options)?;
    let video_info = video.get_info().await?;
    let rytdl = rusty_ytdl::search::YouTube::new_with_options(&request_options)?;
    let metadata = video_info_to_aux_metadata(&video_info);

    let rusty_search = RustyYoutubeSearch {
        rusty_ytdl: rytdl,
        metadata: Some(metadata.clone()),
        query: QueryType::VideoLink(url.clone()),
        url: Some(url),
        video: Some(video),
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
        let req_options = RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        };
        let rytdl = rusty_ytdl::search::YouTube::new_with_options(&req_options)?;

        tracing::warn!("search_query_to_source_and_metadata: {:?}", rytdl);

        // let query = format!("{} {}", query, MUSIC_SEARCH_SUFFIX);
        let query = query.replace("\\", "").replace("\"", "");
        tracing::error!("ACTUALLY SEARCHING FOR THIS: {:?}", query);
        let results = rytdl.search_one(query.clone(), None).await?;

        tracing::warn!("search_query_to_source_and_metadata: {:?}", results);
        // FIXME: Fallback to yt-dlp
        let result = match results {
            Some(r) => r,
            None => {
                return search_query_to_source_and_metadata_ytdl(client, query.to_string()).await
            },
        };
        let metadata = &search_result_to_aux_metadata(&result);
        metadata.clone()
    };

    let source_url = match metadata.clone().source_url {
        Some(url) => url.clone(),
        None => "".to_string(),
    };
    let ytdl = YoutubeDl::new(http_utils::get_client_old().clone(), source_url);
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
    let request_options = RequestOptions {
        client: Some(client.clone()),
        ..Default::default()
    };
    let rusty_yt = rusty_ytdl::search::YouTube::new_with_options(&request_options)?;

    let metadata = {
        tracing::warn!("search_query_to_source_and_metadata_rusty: {:?}", rusty_yt);
        let results = rusty_yt
            .search_one(
                query
                    .build_query()
                    .ok_or(CrackedError::Other("No query given"))?,
                None,
            )
            .await?;
        tracing::warn!("search_query_to_source_and_metadata_rusty: {:?}", results);
        // FIXME: Fallback to yt-dlp
        let result = match results {
            Some(r) => r,
            None => return Err(CrackedError::EmptySearchResult),
        };
        let metadata = &search_result_to_aux_metadata(&result);
        metadata.clone()
    };

    let rusty_search = RustyYoutubeSearch {
        rusty_ytdl: rusty_yt,
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
    _client: reqwest::Client,
    query: String,
) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
    let query = if query.starts_with("ytsearch:") {
        query
    } else {
        format!("ytsearch:{}", query)
    };
    let mut ytdl = YoutubeDl::new(http_utils::get_client_old().clone(), query);
    let metadata = ytdl.aux_metadata().await?;
    let my_metadata = MyAuxMetadata(metadata);

    Ok((ytdl.into(), vec![my_metadata]))
}

/// Build a query from AuxMetadata.
pub fn build_query_aux_metadata(aux_metadata: &AuxMetadata) -> String {
    format!(
        "{} {}",
        aux_metadata.track.clone().unwrap_or_default(),
        aux_metadata.artist.clone().unwrap_or_default(),
    )
}

/// Build a query from AuxMetadata for.
pub fn build_query_lyric_aux_metadata(aux_metadata: &AuxMetadata) -> String {
    format!(
        "{} {} {}",
        aux_metadata.track.clone().unwrap_or_default(),
        aux_metadata.artist.clone().unwrap_or_default(),
        MUSIC_SEARCH_SUFFIX,
    )
}

#[cfg(test)]
mod test {

    use rusty_ytdl::search::YouTube;

    use crate::http_utils::{self};

    use super::*;

    #[test]
    fn test_build_query_aux_metadata() {
        let aux_metadata = AuxMetadata {
            artist: Some("hello".to_string()),
            track: Some("world".to_string()),
            ..Default::default()
        };
        let res = build_query_aux_metadata(&aux_metadata);
        assert_eq!(res, "world hello");
    }

    #[test]
    fn test_build_query_lyric_aux_metadata() {
        let aux_metadata = AuxMetadata {
            artist: Some("hello".to_string()),
            track: Some("world".to_string()),
            ..Default::default()
        };
        let res = build_query_lyric_aux_metadata(&aux_metadata);
        assert_eq!(res, format!("world hello {}", MUSIC_SEARCH_SUFFIX));
    }

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
            // let phrase = "Sign in to confirm you’re not a bot";
            // assert!(e.to_string().contains(phrase));
            println!("{}", e.to_string());
        }
        //assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata() {
        let reqclient = http_utils::get_client().clone();
        let query_type = QueryType::Keywords("hello".to_string());
        let res = query_type
            .get_track_source_and_metadata(Some(reqclient))
            .await;
        if let Err(ref e) = res {
            //let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e.to_string());
            //assert!(e.to_string().contains(phrase));
        }
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_video_link() {
        let query_type =
            QueryType::VideoLink("https://www.youtube.com/watch?v=MNmLn6a-jqw".to_string());
        let client = http_utils::build_client();
        let res = query_type.get_track_source_and_metadata(Some(client)).await;
        if let Err(ref e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e.to_string());
            //assert!(e.to_string());
        }
        //assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_playlist_link() {
        let query_type = QueryType::PlaylistLink(
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".to_string(),
        );
        let client = Some(http_utils::build_client());
        let res = query_type.get_track_source_and_metadata(client).await;
        if let Err(ref e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e.to_string());
            // assert!(e.to_string().contains(phrase));
        }
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_keyword_list() {
        let query_type = QueryType::KeywordList(vec!["hello".to_string(), "world".to_string()]);
        let client = Some(http_utils::build_client());
        let res = query_type.get_track_source_and_metadata(client).await;
        match res {
            Ok(_) => assert!(true),
            Err(e) => {
                // let phrase = "Sign in to confirm you’re not a bot";
                println!("{}", e.to_string());
                // assert!(e.to_string().contains(phrase));
            },
        }
    }

    /// FIXME: Mock the response.
    #[tokio::test]
    async fn test_get_rusty_search() {
        let client = reqwest::Client::new();
        let url = "https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string();
        let res = get_rusty_search(client, url).await;

        match res {
            Ok(search) => assert!(search.metadata.is_some()),
            Err(e) => {
                //let phrase = "Sign in to confirm you’re not a bot";
                //assert!(e.to_string().contains(phrase));
                println!("{}", e.to_string());
            },
        }
    }

    #[tokio::test]
    async fn test_search_query_to_source_and_metadata() {
        let client = reqwest::Client::new();
        let query = "hello".to_string();
        let res = search_query_to_source_and_metadata(client, query).await;
        match res {
            Ok((source, metadata)) => {
                assert!(!source.is_playable());
                assert_eq!(metadata.len(), 1);
            },
            Err(e) => {
                // let phrase = "Sign in to confirm you’re not a bot";
                // assert!(e.to_string().contains(phrase));
                println!("{}", e.to_string());
            },
        }
    }
}
