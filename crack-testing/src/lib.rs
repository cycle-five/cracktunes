use crack_types::AuxMetadata;
use crack_types::Error;
use crack_types::QueryType;
use rusty_ytdl::reqwest;
use rusty_ytdl::{RequestOptions, VideoOptions};
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};

pub use crack_types::{get_human_readable_timestamp, video_info_to_aux_metadata};
pub mod reply_handle_trait;
pub use reply_handle_trait::run as reply_handle_trait_run;

pub const NEW_FAILED: &str = "New failed";

/// Struct the holds a track who's had it's metadata queried,
/// and thus has a video URI associated with it, and it has all the
/// necessary metadata to be displayed in a music player interface.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ResolvedTrack {
    query: QueryType,
    metadata: Option<AuxMetadata>,
    video: Option<rusty_ytdl::Video>,
    details: Option<rusty_ytdl::VideoDetails>,
}

impl ResolvedTrack {
    /// Create a new ResolvedTrack
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            metadata: None,
            video: None,
            details: None,
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

impl Display for ResolvedTrack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let metadata = self.metadata.as_ref().unwrap();
        let title = metadata.title.clone().unwrap_or_default();
        let url = metadata.source_url.clone().unwrap_or_default();
        let duration = get_human_readable_timestamp(metadata.duration);

        write!(f, "[{}]({}) • `{}`", title, url, duration)
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct CrackTrackClient {
    req_client: reqwest::Client,
    yt_client: rusty_ytdl::search::YouTube,
}

impl Default for CrackTrackClient {
    fn default() -> Self {
        let req_client = reqwest::Client::new();
        let opts = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client: rusty_ytdl::search::YouTube::new_with_options(&opts).expect(NEW_FAILED),
        }
    }
}

impl CrackTrackClient {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn new_with_clients(
        req_client: reqwest::Client,
        yt_client: rusty_ytdl::search::YouTube,
    ) -> Self {
        CrackTrackClient {
            req_client,
            yt_client,
        }
    }

    pub fn with_req_client(req_client: reqwest::Client) -> Self {
        let opts = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client: rusty_ytdl::search::YouTube::new_with_options(&opts).expect(NEW_FAILED),
        }
    }

    /// Resolve a track from a query. This does not start or ready the track for playback.
    pub async fn resolve_track(self, query: QueryType) -> Result<ResolvedTrack, Error> {
        let (video, details, metadata) = match query {
            QueryType::VideoLink(ref url) => {
                let request_options = RequestOptions {
                    client: Some(self.req_client.clone()),
                    ..Default::default()
                };
                let video_options = VideoOptions {
                    request_options: request_options.clone(),
                    ..Default::default()
                };
                let video = rusty_ytdl::Video::new_with_options(url, video_options)?;
                let info = video.get_info().await?;
                let metadata = video_info_to_aux_metadata(&info);

                (video, info.video_details, metadata)
            },
            _ => unimplemented!(),
        };
        let track = ResolvedTrack {
            query,
            metadata: Some(metadata),
            video: Some(video),
            details: Some(details),
        };
        Ok(track)
    }
}

/// run function.
pub fn run() {
    let mut queue = VecDeque::new();
    let track = ResolvedTrack {
        query: QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string()),
        metadata: None,
        video: None,
        details: None,
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
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
        };

        let resolved = client.resolve_track(query).await.unwrap();

        let res = resolved.metadata.unwrap();
        assert_eq!(res.title.unwrap(), r#"Molly Nilsson "1995""#.to_string());
    }
}
