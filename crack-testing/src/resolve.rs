use crate::{UNKNOWN_DURATION, UNKNOWN_TITLE, UNKNOWN_URL};
use crack_types::{get_human_readable_timestamp, AuxMetadata, QueryType};
use regex::Regex;
use rusty_ytdl::{search, VideoDetails};
use serenity::all::{AutocompleteChoice, AutocompleteValue, UserId};
use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    time::Duration,
};

static YOUTUBE_URL_REGEX: &str = r"(?im)^((?:https?:)?\/\/)?((?:www|m)\.)?((?:youtube(-nocookie)?\.com|youtu.be))(\/(?:[\w\-]+\?v=|embed\/|v\/)?)([\w\-]+)(\S+)?$";

/// [`ResolvedTrack`] struct for holding resolved track information, this
/// should be enough to play the track or enqueue it with the bot.
#[derive(Clone, Debug)]
pub struct ResolvedTrack<'a> {
    // FIXME One of these three has the possibility of returning
    // the video id instead of the full URL. Need to figure out
    // which one and document why.
    pub details: Option<rusty_ytdl::VideoDetails>,
    pub metadata: Option<AuxMetadata>,
    pub search_video: Option<rusty_ytdl::search::Video>,
    pub query: QueryType,
    pub video: Option<rusty_ytdl::Video<'a>>,
    #[allow(dead_code)]
    pub queued: bool,
    #[allow(dead_code)]
    // requesting user
    pub user_id: UserId,
}

impl Default for ResolvedTrack<'_> {
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

impl ResolvedTrack<'_> {
    /// Create a new `ResolvedTrack`
    #[must_use]
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            user_id: UserId::new(1),
            ..Default::default()
        }
    }

    // ----------------- Setters ----------------- //

    /// Set the user id of the user who requested the track.
    #[must_use]
    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = user_id;
        self
    }

    /// Set the queued status of the track.
    #[must_use]
    pub fn with_queued(mut self, queued: bool) -> Self {
        self.queued = queued;
        self
    }

    /// Set the query type of the track.
    #[must_use]
    pub fn with_query(mut self, query: QueryType) -> Self {
        self.query = query;
        self
    }

    /// Set the details of the track.
    #[must_use]
    pub fn with_details(mut self, details: rusty_ytdl::VideoDetails) -> Self {
        self.details = Some(details);
        self
    }

    /// Set the metadata of the track.
    #[must_use]
    pub fn with_metadata(mut self, metadata: AuxMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the search video of the track.
    #[must_use]
    pub fn with_search_video(mut self, search_video: rusty_ytdl::search::Video) -> Self {
        self.search_video = Some(search_video);
        self
    }

    /// Set the video of the track.
    #[must_use]
    pub fn with_video(mut self, video: rusty_ytdl::Video<'static>) -> Self {
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
            return UNKNOWN_URL.to_string();
        };

        if url.contains("youtube.com") {
            url
        } else {
            format!("https://www.youtube.com/watch?v={url}")
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
        let mut str = format!("{title} ({duration})");
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
    pub fn autocomplete_option(&self) -> AutocompleteChoice<'static> {
        AutocompleteChoice {
            name: Cow::Owned(self.suggest_string()),
            value: AutocompleteValue::String(Cow::Owned(self.get_url())),
            name_localizations: Option::default(),
        }
    }
}

/// Implement [`From`] for [`search::Video`] to [`ResolvedTrack`].
impl From<search::Video> for ResolvedTrack<'_> {
    fn from(video: search::Video) -> Self {
        ResolvedTrack {
            query: QueryType::VideoLink(video.url.clone()),
            search_video: Some(video),
            ..Default::default()
        }
    }
}

/// Implement [`From`] for ([`rusty_ytdl::Video`], [`VideoDetails`], [`AuxMetadata`]) to [`ResolvedTrack`].
impl<'a> From<(rusty_ytdl::Video<'a>, VideoDetails, AuxMetadata)> for ResolvedTrack<'a> {
    fn from(
        (video, video_details, aux_metadata): (rusty_ytdl::Video<'a>, VideoDetails, AuxMetadata),
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
impl Display for ResolvedTrack<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let title = self.get_title();
        let url = self.get_url();
        let duration = self.get_duration();

        write!(f, "[{title}]({url}) • `{duration}`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_types::{build_fake_rusty_video_details, build_fake_search_video};
    use std::time::Duration;

    fn create_mock_video_details() -> VideoDetails {
        build_fake_rusty_video_details()
    }

    fn create_mock_search_video() -> search::Video {
        build_fake_search_video()
    }

    fn create_mock_aux_metadata() -> AuxMetadata {
        AuxMetadata {
            title: Some("Metadata Test Video".to_string()),
            source_url: Some("https://www.youtube.com/watch?v=meta123".to_string()),
            duration: Some(Duration::from_secs(300)), // 5 minutes
            ..Default::default()
        }
    }

    #[test]
    fn test_default() {
        let track = ResolvedTrack::default();
        assert_eq!(track.get_title(), UNKNOWN_TITLE);
        assert_eq!(track.get_url(), UNKNOWN_URL);
        assert_eq!(track.get_duration(), UNKNOWN_DURATION);
        assert!(track.get_metadata().is_none());
        assert!(track.get_video().is_none());
    }

    #[test]
    fn test_new() {
        let query = QueryType::VideoLink("test_url".to_string());
        let track = ResolvedTrack::new(query.clone());
        // Can't compare QueryType directly since it doesn't implement PartialEq
        match track.query {
            QueryType::VideoLink(url) => assert_eq!(url, "test_url"),
            _ => panic!("Wrong query type"),
        }
        assert_eq!(track.user_id, UserId::new(1));
    }

    #[test]
    fn test_with_methods() {
        let track = ResolvedTrack::default()
            .with_user_id(UserId::new(123))
            .with_queued(true)
            .with_query(QueryType::VideoLink("test".to_string()));

        assert_eq!(track.user_id, UserId::new(123));
        assert!(track.queued);
        if let QueryType::VideoLink(url) = track.query {
            assert_eq!(url, "test");
        } else {
            panic!("Wrong query type");
        }
    }

    #[test]
    fn test_get_title_priority() {
        let search_video = create_mock_search_video();
        let video_details = create_mock_video_details();
        let aux_metadata = create_mock_aux_metadata();

        // Test search_video priority
        let track = ResolvedTrack::default()
            .with_search_video(search_video.clone())
            .with_details(video_details.clone())
            .with_metadata(aux_metadata.clone());
        assert_eq!(track.get_title(), search_video.title);

        // Test metadata priority when no search_video
        let track = ResolvedTrack::default()
            .with_details(video_details.clone())
            .with_metadata(aux_metadata.clone());
        assert_eq!(track.get_title(), aux_metadata.title.unwrap());

        // Test details priority when no search_video or metadata
        let track = ResolvedTrack::default().with_details(video_details.clone());
        assert_eq!(track.get_title(), video_details.title);
    }

    #[test]
    fn test_get_url_priority() {
        let search_video = create_mock_search_video();
        let video_details = create_mock_video_details();
        let aux_metadata = create_mock_aux_metadata();

        // Test search_video priority
        let track = ResolvedTrack::default()
            .with_search_video(search_video.clone())
            .with_details(video_details.clone())
            .with_metadata(aux_metadata.clone());
        assert_eq!(track.get_url(), search_video.url);

        // Test metadata priority when no search_video
        let track = ResolvedTrack::default()
            .with_details(video_details.clone())
            .with_metadata(aux_metadata.clone());
        assert_eq!(track.get_url(), aux_metadata.source_url.unwrap());

        // Test details priority when no search_video or metadata
        let track = ResolvedTrack::default().with_details(video_details.clone());
        assert_eq!(track.get_url(), video_details.video_url);
    }

    #[test]
    fn test_get_duration_priority() {
        let search_video = create_mock_search_video();
        let video_details = create_mock_video_details();
        let aux_metadata = create_mock_aux_metadata();

        // Test metadata priority
        let track = ResolvedTrack::default()
            .with_search_video(search_video.clone())
            .with_details(video_details.clone())
            .with_metadata(aux_metadata.clone());
        assert_eq!(
            track.get_duration(),
            get_human_readable_timestamp(aux_metadata.duration)
        );

        // Test details priority when no metadata
        let track = ResolvedTrack::default()
            .with_search_video(search_video.clone())
            .with_details(video_details.clone());
        assert_eq!(
            track.get_duration(),
            get_human_readable_timestamp(Some(Duration::from_secs(60)))
        );

        // // Test search_video priority when no metadata or details
        // let track = ResolvedTrack::default().with_search_video(search_video.clone());
        // assert_eq!(
        //     track.get_duration(),
        //     get_human_readable_timestamp(Some(Duration::from_millis(1440000)))
        // );
    }

    #[test]
    fn test_suggest_string() {
        let video_details = create_mock_video_details();
        let track = ResolvedTrack::default().with_details(video_details);
        let suggestion = track.suggest_string();
        assert!(suggestion.contains("Title"));
        //assert!(suggestion.contains("3:00")); // 180 seconds formatted
    }

    #[test]
    fn test_suggest_string_truncation() {
        let mut video_details = create_mock_video_details();
        video_details.title = "A".repeat(200); // Very long title
        let track = ResolvedTrack::default().with_details(video_details);
        let suggestion = track.suggest_string();
        assert!(suggestion.len() <= 100);
    }

    #[test]
    fn test_autocomplete_option() {
        let video_details = create_mock_video_details();
        let track = ResolvedTrack::default().with_details(video_details);
        let option = track.autocomplete_option();
        assert!(option.name.contains("Title"));
        assert!(matches!(
            option.value,
            AutocompleteValue::String(ref s) if s.contains("youtube.com")
        ));
    }

    #[test]
    fn test_from_search_video() {
        let search_video = create_mock_search_video();
        let track = ResolvedTrack::from(search_video.clone());
        assert_eq!(track.get_title(), search_video.title);
        assert_eq!(track.get_url(), search_video.url);
        if let QueryType::VideoLink(url) = track.query {
            assert_eq!(url, search_video.url);
        } else {
            panic!("Wrong query type");
        }
    }

    #[test]
    fn test_display() {
        let video_details = create_mock_video_details();
        let track = ResolvedTrack::default().with_details(video_details);
        let display = format!("{track}");
        assert!(display.contains("Title"));
        //assert!(display.contains("youtube.com"));
    }

    #[test]
    fn test_regex1() {
        let regex = Regex::new(r"(?im)^((?:https?:)?\/\/)?((?:www|m)\.)?((?:youtube(-nocookie)?\.com|youtu.be))(\/(?:[\w\-]+\?v=|embed\/|v\/)?)([\w\-]+)(\S+)?$").unwrap();
        let string = r#"https://www.youtube.com/watch?v=DFYRQ_zQ-gk&feature=featured
https://www.youtube.com/watch?v=DFYRQ_zQ-gk
http://www.youtube.com/watch?v=DFYRQ_zQ-gk
//www.youtube.com/watch?v=DFYRQ_zQ-gk
www.youtube.com/watch?v=DFYRQ_zQ-gk
https://youtube.com/watch?v=DFYRQ_zQ-gk
http://youtube.com/watch?v=DFYRQ_zQ-gk
//youtube.com/watch?v=DFYRQ_zQ-gk
youtube.com/watch?v=DFYRQ_zQ-gk

https://m.youtube.com/watch?v=DFYRQ_zQ-gk
http://m.youtube.com/watch?v=DFYRQ_zQ-gk
//m.youtube.com/watch?v=DFYRQ_zQ-gk
m.youtube.com/watch?v=DFYRQ_zQ-gk

https://www.youtube.com/v/DFYRQ_zQ-gk?fs=1&hl=en_US
http://www.youtube.com/v/DFYRQ_zQ-gk?fs=1&hl=en_US
//www.youtube.com/v/DFYRQ_zQ-gk?fs=1&hl=en_US
www.youtube.com/v/DFYRQ_zQ-gk?fs=1&hl=en_US
youtube.com/v/DFYRQ_zQ-gk?fs=1&hl=en_US

https://www.youtube.com/embed/DFYRQ_zQ-gk?autoplay=1
https://www.youtube.com/embed/DFYRQ_zQ-gk
http://www.youtube.com/embed/DFYRQ_zQ-gk
//www.youtube.com/embed/DFYRQ_zQ-gk
www.youtube.com/embed/DFYRQ_zQ-gk
https://youtube.com/embed/DFYRQ_zQ-gk
http://youtube.com/embed/DFYRQ_zQ-gk
//youtube.com/embed/DFYRQ_zQ-gk
youtube.com/embed/DFYRQ_zQ-gk

https://www.youtube-nocookie.com/embed/DFYRQ_zQ-gk?autoplay=1
https://www.youtube-nocookie.com/embed/DFYRQ_zQ-gk
http://www.youtube-nocookie.com/embed/DFYRQ_zQ-gk
//www.youtube-nocookie.com/embed/DFYRQ_zQ-gk
www.youtube-nocookie.com/embed/DFYRQ_zQ-gk
https://youtube-nocookie.com/embed/DFYRQ_zQ-gk
http://youtube-nocookie.com/embed/DFYRQ_zQ-gk
//youtube-nocookie.com/embed/DFYRQ_zQ-gk
youtube-nocookie.com/embed/DFYRQ_zQ-gk

https://youtu.be/DFYRQ_zQ-gk?t=120
https://youtu.be/DFYRQ_zQ-gk
http://youtu.be/DFYRQ_zQ-gk
//youtu.be/DFYRQ_zQ-gk
youtu.be/DFYRQ_zQ-gk

https://www.youtube.com/HamdiKickProduction?v=DFYRQ_zQ-gk"#;

        let result = regex.captures_iter(string);

        let num_matches = result.count();
        println!("{:?}", num_matches);
        assert!(num_matches > 0);
    }
}
