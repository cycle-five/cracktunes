use crack_types::AuxMetadata;
use crack_types::QueryType;
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

    /// Get the metadata for the track.
    pub async fn aux_metadata(&self) -> Vec<AuxMetadata> {
        if let Some(metadata) = &self.metadata {
            vec![metadata.clone()]
        } else {
            let metadata = self
                .query
                .get_track_metadata()
                .await
                .unwrap_or_else(|_| Vec::new());
            self.metadata = metadata.first().cloned();
            vec![self.metadata.clone()];
        }
    }
}

#[derive(Clone, Debug)]
pub struct CrackTrackClient {
    reqclient: reqwest::Client,
    ytclient: rusty_ytdl::search::YouTube,
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
    async fn test_aux_metadata() {
        let track = ResolvedTrack::new(QueryType::VideoLink(
            "https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string(),
        ));
        let metadata = track.aux_metadata().await;
        assert_eq!(metadata.len(), 1);
        let metadata = track.aux_metadata().await;
        if let Some(title) = &metadata.first().unwrap().title {
            assert_eq!(title, "NOTITLE");
        }
    }
}
