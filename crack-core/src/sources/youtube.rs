use crate::errors::CrackedError;
use crate::http_utils;
use crate::music::query::NewQueryType;
use crack_types::QueryType;

use crate::utils::MUSIC_SEARCH_SUFFIX;
use crack_types::metadata::search_result_to_aux_metadata;
use crack_types::NewAuxMetadata;
use crack_types::{or_ask_ytdlp, ytdl_for_url};
use rusty_ytdl::RequestOptions;
use songbird::input::{AuxMetadata, Input as SongbirdInput, YoutubeDl};

/// Search youtube for a query and return the source (playable)
/// and metadata.
pub async fn search_query_to_source_and_metadata(
    client: reqwest::Client,
    query: String,
) -> Result<(SongbirdInput, Vec<NewAuxMetadata>), CrackedError> {
    tracing::warn!("search_query_to_source_and_metadata: {:?}", query);

    // let query = format!("{} {}", query, MUSIC_SEARCH_SUFFIX);
    let query = query.replace("\\", "").replace("\"", "");
    let rusty = rusty_search_hit(client, &query).await;
    let metadata = or_ask_ytdlp(&query, rusty, || ytdlp_search_hit(&query)).await?;
    source_for_search_hit(metadata)
}

/// rusty_ytdl's top hit for `query`, as metadata.
async fn rusty_search_hit(
    client: reqwest::Client,
    query: &str,
) -> Result<Option<AuxMetadata>, CrackedError> {
    let req_options = RequestOptions {
        client: Some(client),
        ..Default::default()
    };
    let rytdl = rusty_ytdl::search::YouTube::new_with_options(&req_options)?;
    let result = rytdl.search_one(query, None).await?;
    Ok(result.as_ref().map(search_result_to_aux_metadata))
}

/// yt-dlp's top hit for `query`, as metadata.
///
/// 🔒 `new_search`, never `YoutubeDl::new`: its `ytsearch1:` prefix keeps the
/// text from ever reaching yt-dlp as an option.
async fn ytdlp_search_hit(query: &str) -> Result<AuxMetadata, CrackedError> {
    let mut ytdl = YoutubeDl::new_search(http_utils::get_client_old().clone(), query.to_string());
    ytdl.search(Some(1))
        .await?
        .next()
        .ok_or(CrackedError::EmptySearchResult)
}

/// Search youtube for a query and return the source (playable)
/// and metadata.
pub async fn search_query_to_source_and_metadata_rusty(
    client: reqwest::Client,
    query: QueryType,
) -> Result<(SongbirdInput, Vec<NewAuxMetadata>), CrackedError> {
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
                NewQueryType(query.clone())
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

    source_for_search_hit(metadata)
}

/// The playable source for a video a search found, with the hit's metadata.
///
/// 🪤 yt-dlp plays it, whatever found it. rusty_ytdl's own stream comes back
/// empty (see `build_track`), and `/play`'s fallback played through it until
/// v0.12.1: the track resolved, queued, and was dropped the moment it started.
pub(crate) fn source_for_search_hit(
    metadata: AuxMetadata,
) -> Result<(SongbirdInput, Vec<NewAuxMetadata>), CrackedError> {
    let url = metadata.source_url.clone().ok_or(CrackedError::Other(
        "a search hit with no URL has nothing to play",
    ))?;
    let source = ytdl_for_url(http_utils::get_client_old().clone(), &url)?;
    Ok((source.into(), vec![NewAuxMetadata(metadata)]))
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

    /// Nothing listens on port 9, so whatever opens a source pointing here
    /// fails at once, without the network.
    const UNREACHABLE: &str = "http://127.0.0.1:9/nothing";

    /// 🪤 rusty_ytdl finds videos but cannot play them: the googlevideo URL it
    /// hands back 403s, songbird gets an empty stream, and symphonia reports
    /// "no suitable format reader found". `/play`'s fallback played through it
    /// until v0.12.1, so a play that reached the fallback resolved, queued, and
    /// was silently dropped. Opening a hit's source must run yt-dlp.
    #[tokio::test]
    async fn a_search_hit_plays_through_yt_dlp() {
        let hit = AuxMetadata {
            title: Some("I Wanna Be in the Cavalry".to_owned()),
            source_url: Some(UNREACHABLE.to_owned()),
            ..Default::default()
        };

        let (source, metadata) = source_for_search_hit(hit).expect("a hit with a URL has a source");

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].0.source_url.as_deref(), Some(UNREACHABLE));
        let SongbirdInput::Lazy(mut source) = source else {
            panic!("a hit's source is opened when it plays, not while it is queued");
        };
        let Err(err) = source.create_async().await else {
            panic!("nothing listens on port 9, so opening the source cannot succeed");
        };
        assert!(
            err.to_string().contains("yt-dlp"),
            "the source was opened by something other than yt-dlp: {err}"
        );
    }

    /// A hit with no URL has nothing to play. Queueing it anyway is a track
    /// that fails at play time and vanishes from the queue.
    #[test]
    fn a_search_hit_without_a_url_is_an_error() {
        let hit = AuxMetadata {
            title: Some("I Wanna Be in the Cavalry".to_owned()),
            ..Default::default()
        };

        assert!(source_for_search_hit(hit).is_err());
    }

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
        let query_type = NewQueryType(query_type);
        let res = query_type.get_track_metadata(ytclient, reqclient).await;
        if let Err(ref e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            // assert!(e.to_string().contains(phrase));
            println!("{}", e);
        }
        //assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata() {
        let reqclient = http_utils::get_client().clone();
        let query_type = QueryType::Keywords("hello".to_string());
        let query_type = NewQueryType(query_type);
        let res = query_type
            .get_track_source_and_metadata(Some(reqclient))
            .await;
        if let Err(ref e) = res {
            //let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e);
            //assert!(e.to_string().contains(phrase));
        }
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_video_link() {
        let query_type =
            QueryType::VideoLink("https://www.youtube.com/watch?v=MNmLn6a-jqw".to_string());
        let query_type = NewQueryType(query_type);
        let client = http_utils::build_client();
        let res = query_type.get_track_source_and_metadata(Some(client)).await;
        if let Err(ref e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e);
            //assert!(e.to_string());
        }
        //assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_playlist_link() {
        let query_type = QueryType::PlaylistLink(
            "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".to_string(),
        );
        let query_type = NewQueryType(query_type);
        let client = Some(http_utils::build_client());
        let res = query_type.get_track_source_and_metadata(client).await;
        if let Err(ref e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e);
            // assert!(e.to_string().contains(phrase));
        }
    }

    #[tokio::test]
    async fn test_get_track_source_and_metadata_keyword_list() {
        let query_type = NewQueryType(QueryType::KeywordList(vec![
            "hello".to_string(),
            "world".to_string(),
        ]));
        let client = Some(http_utils::build_client());
        let res = query_type.get_track_source_and_metadata(client).await;
        if let Err(e) = res {
            // let phrase = "Sign in to confirm you’re not a bot";
            println!("{}", e);
            // assert!(e.to_string().contains(phrase));
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
                println!("{}", e);
            },
        }
    }
}
