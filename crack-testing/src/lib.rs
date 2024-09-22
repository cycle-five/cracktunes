use crack_types::AuxMetadata;
use crack_types::Error;
use crack_types::QueryType;
use rusty_ytdl::reqwest;
use std::collections::VecDeque;

pub mod reply_handle_trait;
pub use reply_handle_trait::run as reply_handle_trait_run;

/// Struct the holds a track who's had it's metadata queried,
/// and thus has a video URI associated with it, and it has all the
/// necessary metadata to be displayed in a music player interface.
pub struct ResolvedTrack {
    query: QueryType,
    metadata: Option<AuxMetadata>,
    video: Option<rusty_ytdl::Video>,
}

impl ResolvedTrack {
    /// Create a new ResolvedTrack
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            metadata: None,
            video: None,
        }
    }

    // /// Get the metadata for the track.
    // pub async fn aux_metadata(&self) -> Result<AuxMetadata, Error> {
    //     if let Some(metadata) = &self.metadata {
    //         Ok(metadata.clone())
    //     } else {
    //         let metadata = match self.query.get_track_metadata().await {
    //             Ok(metadata) => metadata,
    //             Err(e) => {
    //                 return Err(Box::new(e));
    //             },
    //         };
    //         self.metadata = metadata.first().cloned();
    //         Ok(self.metadata.clone())
    //     }
    // }
}

pub async fn resolve_track(
    client: CrackTrackClient,
    query: QueryType,
) -> Result<ResolvedTrack, Error> {
    let (video, info, metadata) = match query {
        QueryType::VideoLink(link) => {
            let video = rusty_ytdl::Video::new(&link)?;
            let info = video.get_info().await?;
            let metadata = video_info_to_aux_metadata(info)?;

            (video, info, metadata)
        },
        // QueryType::SearchQuery(query) => {
        //     let videos = client.yt_client.search(query).await?;
        //     let mut metadata = Vec::new();
        //     for video in videos {
        //         let metadata = video.get_metadata().await?;
        //         metadata.push(metadata);
        //     }
        //     metadata
        // },
        _ => unimplemented!(),
    };
    let track = ResolvedTrack {
        query,
        metadata: Some(metadata),
        video: Some(video),
    };
    Ok(track)
}

#[derive(Clone, Debug)]
pub struct CrackTrackClient {
    req_client: reqwest::Client,
    yt_client: rusty_ytdl::search::YouTube,
}

/// run function.
pub fn run() {
    let mut queue = VecDeque::new();
    let mut track = ResolvedTrack {
        query: QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string()),
        metadata: None,
        video: None,
    };

    queue.push_back(track);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run() {
        run();
    }

    #[test]
    fn test_new() {
        let track = ResolvedTrack::new(QueryType::VideoLink(
            "https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string(),
        ));
        assert_eq!(track.metadata, None);
        assert_eq!(track.video, None);
    }

    #[tokio::test]
    async fn test_resolve_track() {
        let query = QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string());
        let client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect("New failed"),
        };

        let resolved = resolve_track(client, query).await.unwrap();

        let res = resolved.metadata.unwrap();
        assert_eq!(res.title.unwrap(), r#"Molly Nilsson "1995""#.to_string());
    }
}
