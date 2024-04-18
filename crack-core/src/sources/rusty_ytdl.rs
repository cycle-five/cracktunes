use std::{fmt::Display, time::Duration};

use crate::{commands::QueryType, errors::CrackedError};
use rusty_ytdl::{
    search::{Playlist, SearchOptions, SearchResult, YouTube},
    RequestOptions, Video, VideoInfo, VideoOptions,
};
use songbird::input::AuxMetadata;

/// Out strucut to wrap the rusty-ytdl search instance
//TODO expand to go beyond search
#[derive(Clone, Debug)]
pub struct RustyYoutubeClient {
    pub rusty_ytdl: YouTube,
    pub client: reqwest::Client,
}

impl Display for RustyYoutubeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RustyYoutubeClient({:?}, {:?})",
            self.rusty_ytdl, self.client
        )
    }
}

#[derive(Clone, Debug)]
pub struct RustyYoutubeSearch {
    rusty_ytdl: RustyYoutubeClient,
    metadata: Option<AuxMetadata>,
    query: QueryType,
}

impl Display for RustyYoutubeSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RustyYoutubeSearch({:?}, {:?}, {:?})",
            self.rusty_ytdl, self.metadata, self.query
        )
    }
}

/// Implementation of the `RustyYoutubeClient` struct.
impl RustyYoutubeClient {
    // Create a new `RustyYoutubeClient`.
    pub fn new() -> Result<Self, CrackedError> {
        let client = reqwest::Client::new();
        RustyYoutubeClient::new_with_client(client)
    }

    /// Creates a new instance of `RustyYoutubeClient`. Requires a `reqwest::Client` instance, preferably reused.
    pub fn new_with_client(client: reqwest::Client) -> Result<Self, CrackedError> {
        let rusty_ytdl = YouTube::new_with_options(&RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        })?;
        Ok(Self { rusty_ytdl, client })
    }

    /// Get a Playlist from a URL.
    pub async fn get_playlist(&self, url: String) -> Result<Playlist, CrackedError> {
        let playlist = Playlist::get(&url, None).await?;
        Ok(playlist)
    }

    /// Convert a `SearchResult` to `AuxMetadata`.
    pub fn search_result_to_aux_metadata(res: &SearchResult) -> AuxMetadata {
        let mut metadata = AuxMetadata::default();
        match res.clone() {
            SearchResult::Video(video) => {
                metadata.track = Some(video.title.clone());
                metadata.artist = None;
                metadata.album = None;
                metadata.date = video.uploaded_at;

                metadata.channels = Some(2);
                metadata.channel = Some(video.channel.name);
                metadata.duration = Some(Duration::from_millis(video.duration));
                metadata.sample_rate = Some(48000);
                metadata.source_url = Some(video.url);
                metadata.title = Some(video.title);
                metadata.thumbnail = Some(video.thumbnails.first().unwrap().url.clone());
            },
            SearchResult::Playlist(playlist) => {
                metadata.title = Some(playlist.name);
                metadata.source_url = Some(playlist.url);
                metadata.duration = None;
                metadata.thumbnail = Some(playlist.thumbnails.first().unwrap().url.clone());
            },
            _ => {},
        };
        metadata
    }

    pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
        let mut metadata = AuxMetadata::default();
        let details = &video.video_details;
        metadata.track = Some(details.title.clone());
        metadata.artist = None;
        metadata.album = None;
        metadata.date = Some(details.publish_date.clone());

        metadata.channels = Some(2);
        metadata.channel = Some(details.owner_channel_name.clone());
        metadata.duration = Some(Duration::from_secs(
            details.length_seconds.parse::<u64>().unwrap_or_default(),
        ));
        metadata.sample_rate = Some(48000);
        metadata.source_url = Some(details.video_url.clone());
        metadata.title = Some(details.title.clone());
        metadata.thumbnail = Some(details.thumbnails.first().unwrap().url.clone());

        metadata
    }

    /// Get a video from a URL.
    pub async fn get_video_info(&self, url: String) -> Result<VideoInfo, CrackedError> {
        let vid_options = VideoOptions {
            request_options: RequestOptions {
                client: Some(self.client.clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        let video = Video::new_with_options(&url, vid_options)?;
        video.get_basic_info().await.map_err(|e| e.into())
    }

    // Search youtube
    pub async fn search(
        &self,
        query: String,
        limit: u64,
    ) -> Result<Vec<SearchResult>, CrackedError> {
        let opts = SearchOptions {
            limit,
            ..Default::default()
        };
        let search_results = self.rusty_ytdl.search(&query, Some(&opts)).await?;
        println!("{:?}", search_results);
        Ok(search_results)
    }

    // Do a one-shot search
    pub async fn one_shot(&self, query: String) -> Result<Vec<SearchResult>, CrackedError> {
        let opts = SearchOptions {
            limit: 1,
            ..Default::default()
        };
        let search_results = self.rusty_ytdl.search(&query, Some(&opts)).await?;
        println!("{:?}", search_results);
        Ok(search_results)
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use songbird::input::YoutubeDl;

    #[tokio::test]
    async fn test_ytdl() {
        // let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let client = reqwest::Client::new();
        let ytdl = crate::sources::rusty_ytdl::RustyYoutubeClient::new_with_client(client).unwrap();
        let ytdl = Arc::new(ytdl);
        let playlist = ytdl
            .one_shot("The Night Chicago Died".to_string())
            .await
            .unwrap();
        println!("{:?}", playlist);
    }

    #[tokio::test]
    async fn test_rusty_ytdl_serial() {
        // let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];
        let ytdl = crate::sources::rusty_ytdl::RustyYoutubeClient::new().unwrap();
        let ytdl = Arc::new(ytdl);
        let mut all_res = Vec::new();
        for search in searches {
            let res = ytdl.one_shot(search.to_string()).await.unwrap();
            assert!(res.len() > 0);
            all_res.push(res.first().unwrap().clone());
        }
        println!("{:?}", all_res);
    }

    #[tokio::test]
    async fn test_ytdl_serial() {
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];
        let mut res_all = Vec::with_capacity(searches.len());
        let client = reqwest::Client::new();
        for search in searches {
            let mut ytdl = YoutubeDl::new_search(client.clone(), search.to_string());
            let res = &mut ytdl.search(Some(1)).await.unwrap();
            res_all.append(res);
        }

        assert!(res_all.len() == 5);

        println!("{:?}", res_all);
    }
    // #[tokio::test]
    // async fn test_ytdl_parallel() {
    //     // let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
    //     let searches = vec![
    //         "The Night Chicago Died".to_string(),
    //         "The Devil Went Down to Georgia".to_string(),
    //         "Hit That The Offspring".to_string(),
    //         "Nightwish I Wish I had an Angel".to_string(),
    //         "Oh Shit I'm Feeling It".to_string(),
    //     ];
    //     let ytdl = crate::sources::rusty_ytdl::MyRustyYoutubeDl::new(None).unwrap();
    //     let ytdl = Arc::new(ytdl);
    //     use tokio::task::JoinSet;

    //     {
    //         let ytdl2 = ytdl.clone();
    //         let mut futures = Vec::with_capacity(searches.len());
    //         for search in searches {
    //             let fut = ytdl2.clone().one_shot(search);
    //             futures.push(fut);
    //         }

    //         let mut set = JoinSet::new();

    //         for fut in futures {
    //             set.spawn(fut);
    //         }

    //         let mut results = Vec::with_capacity(futures.len());
    //         while let Some(res) = set.join_next().await {
    //             let out = &mut res.unwrap().unwrap();
    //             results.append(out);
    //         }

    //         assert!(results.len() == 5);
    //         println!("{:?}", results);
    //     }
    //     println!("{:?}", ytdl)
    //     // for search in searches {
    //     //     let join_handle =
    //     //         tokio::spawn(async move { ytdl.clone().one_shot(search.to_string()) });
    //     //     handles.push(join_handle);
    //     // }

    //     // for handle in handles {
    //     //     results.push(handle.await.unwrap())
    //     // }
    //     //tokio::join!(all_res);
    // }
}
