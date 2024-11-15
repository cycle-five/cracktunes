use crate::{UNKNOWN_DURATION, UNKNOWN_TITLE, UNKNOWN_URL};
use crack_types::{get_human_readable_timestamp, AuxMetadata, QueryType};
use rusty_ytdl::{search, VideoDetails};
use serenity::all::{AutocompleteChoice, AutocompleteValue, UserId};
use std::{
    borrow::Cow, fmt::{self, Display, Formatter}, time::Duration
};

/// [`ResolvedTrack`] struct for holding resolved track information, this
/// should be enough to play the track or enqueue it with the bot.
#[derive(Clone, Debug)]
pub struct ResolvedTrack {
    // FIXME One of these three has the possibility of returning
    // the video id instead of the full URL. Need to figure out
    // which one and document why.
    pub details: Option<rusty_ytdl::VideoDetails>,
    pub metadata: Option<AuxMetadata>,
    pub search_video: Option<rusty_ytdl::search::Video>,
    pub query: QueryType,
    pub video: Option<rusty_ytdl::Video>,
    #[allow(dead_code)]
    pub queued: bool,
    #[allow(dead_code)]
    // requesting user
    pub user_id: UserId,
}

impl Default for ResolvedTrack {
    fn default() -> Self {
        ResolvedTrack {
            query: QueryType::None,
            user_id: UserId::new(1),
            details: None,
            metadata: None,
            search_video: None,
            video: None,
            queued: false,
        }
    }
}

impl ResolvedTrack {
    /// Create a new ResolvedTrack
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            user_id: UserId::new(1),
            ..Default::default()
        }
    }

    // ----------------- Setters ----------------- //

    /// Set the user id of the user who requested the track.
    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = user_id;
        self
    }

    /// Set the queued status of the track.
    pub fn with_queued(mut self, queued: bool) -> Self {
        self.queued = queued;
        self
    }

    /// Set the query type of the track.
    pub fn with_query(mut self, query: QueryType) -> Self {
        self.query = query;
        self
    }

    /// Set the details of the track.
    pub fn with_details(mut self, details: rusty_ytdl::VideoDetails) -> Self {
        self.details = Some(details);
        self
    }

    /// Set the metadata of the track.
    pub fn with_metadata(mut self, metadata: AuxMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the search video of the track.
    pub fn with_search_video(mut self, search_video: rusty_ytdl::search::Video) -> Self {
        self.search_video = Some(search_video);
        self
    }

    /// Set the video of the track.
    pub fn with_video(mut self, video: rusty_ytdl::Video) -> Self {
        self.video = Some(video);
        self
    }

    // ----------------- Getters ----------------- //

    /// Get the title of the track.
    pub fn get_title(&self) -> String {
        if let Some(search_video) = &self.search_video {
            search_video.title.clone()
        } else if let Some(metadata) = &self.metadata {
            metadata.title.clone().unwrap_or_default()
        } else if let Some(details) = &self.details {
            details.title.clone()
        } else {
            UNKNOWN_TITLE.to_string()
        }
    }

    /// Get the URL of the track.
    pub fn get_url(&self) -> String {
        let url = if let Some(search_video) = &self.search_video {
            search_video.url.clone()
        } else if let Some(metadata) = &self.metadata {
            metadata.source_url.clone().unwrap_or_default()
        } else if let Some(details) = &self.details {
            details.video_url.clone()
        } else {
            UNKNOWN_URL.to_string()
        };

        if url.contains("youtube.com") {
            url
        } else {
            format!("https://www.youtube.com/watch?v={}", url)
        }
    }

    /// Get the duration of the track.
    pub fn get_duration(&self) -> String {
        if let Some(metadata) = &self.metadata {
            get_human_readable_timestamp(metadata.duration)
        } else if let Some(details) = &self.details {
            let duration =
                Duration::from_secs(details.length_seconds.parse::<u64>().unwrap_or_default());
            get_human_readable_timestamp(Some(duration))
        } else if let Some(search_video) = &self.search_video {
            let duration = Duration::from_millis(search_video.duration);
            get_human_readable_timestamp(Some(duration))
        } else {
            UNKNOWN_DURATION.to_string()
        }
    }

    /// Get the metadata of the track.
    pub fn get_metadata(&self) -> Option<AuxMetadata> {
        self.metadata.clone()
    }

    /// Return the user id of the user who requested the track.
    pub fn get_requesting_user(&self) -> UserId {
        self.user_id
    }

    /// Get the video object if it exists.
    pub fn get_video(&self) -> Option<rusty_ytdl::Video> {
        self.video.clone()
    }

    /// Get the autocomplete suggestion string for the track.
    pub fn suggest_string(&self) -> String {
        let title = self.get_title();
        //let url = self.get_url();
        let duration = self.get_duration();
        let dur_len = duration.len() + 3;
        let mut str = format!("{} ({})", title, duration);
        let len = str.len();
        if len > 100 - dur_len {
            let mut truncate_index = 100 - dur_len;
            while !str.is_char_boundary(truncate_index) {
                truncate_index -= 1;
            }
            str.truncate(100 - dur_len);
        }
        str
    }

    /// autocomplete option for the track.
    pub fn autocomplete_option(&self) -> AutocompleteChoice {
        AutocompleteChoice {
            name: Cow::Owned(self.suggest_string()),
            value: AutocompleteValue::String(Cow::Owned((self.get_url()))),
            name_localizations: Default::default(),
        }
    }
}

// impl From<ResolvedTrack> for songbird::Input {
//     fn from(track: ResolvedTrack) -> Self {
//         let client = REQ_CLIENT.clone();
//         let ytdl = YoutubeDl::new(client, track.get_url());
//         songbird::Input::from(ytdl)
//     }
// }

/// Implement [`From``] for [`search::Video`] to [`ResolvedTrack`].
impl From<search::Video> for ResolvedTrack {
    fn from(video: search::Video) -> Self {
        ResolvedTrack {
            query: QueryType::VideoLink(video.url.clone()),
            search_video: Some(video),
            ..Default::default()
        }
    }
}

/// Implement [`From`] for ([`rusty_ytdl::Video`], [`VideoDetails`], [`AuxMetadata`]) to [`ResolvedTrack`].
impl From<(rusty_ytdl::Video, VideoDetails, AuxMetadata)> for ResolvedTrack {
    fn from(
        (video, video_details, aux_metadata): (rusty_ytdl::Video, VideoDetails, AuxMetadata),
    ) -> Self {
        ResolvedTrack {
            query: QueryType::VideoLink(video.get_video_url()),
            video: Some(video),
            metadata: Some(aux_metadata),
            details: Some(video_details),
            ..Default::default()
        }
    }
}

/// Implement [`Display`] for [`ResolvedTrack`].
impl Display for ResolvedTrack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let title = self.get_title();
        let url = self.get_url();
        let duration = self.get_duration();

        write!(f, "[{}]({}) • `{}`", title, url, duration)
    }
}
