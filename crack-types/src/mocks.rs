use rusty_ytdl::search::Channel as RustyYtChannel;
use rusty_ytdl::search::Video as RustyYtVideo;
use rusty_ytdl::Thumbnail as RustyYtThumbnail;
use serenity::all::Token;

/// Builds a mock [`RustyYtVideo`] for testing purposes.
#[must_use]
pub fn build_mock_search_video() -> RustyYtVideo {
    RustyYtVideo {
        id: "id".to_string(),
        title: "title".to_string(),
        description: "description".to_string(),
        duration: 14400,
        thumbnails: build_mock_thumbnails(),
        channel: RustyYtChannel {
            id: "id".to_string(),
            name: "name".to_string(),
            url: "url".to_string(),
            verified: false,
            subscribers: 0,
            icon: build_mock_thumbnails(),
        },
        views: 0,
        url: "youtube.com".to_string(),
        duration_raw: "60".to_string(),
        uploaded_at: Some("uploaded_at".to_string()),
    }
}

/// Builds a mock [`RustyYtChannel`] for testing purposes.
#[must_use]
pub fn build_mock_thumbnails() -> Vec<RustyYtThumbnail> {
    vec![RustyYtThumbnail {
        url: "thumbnail_url".to_string(),
        width: 0,
        height: 0,
    }]
}

/// Builds a mock [`rusty_ytdl::Author`] for testing purposes.
#[must_use]
pub fn build_mock_rusty_author() -> rusty_ytdl::Author {
    rusty_ytdl::Author {
        id: "id".to_string(),
        name: "name".to_string(),
        user: "user".to_string(),
        channel_url: "channel_url".to_string(),
        external_channel_url: "external_channel_url".to_string(),
        user_url: "user_url".to_string(),
        thumbnails: vec![],
        verified: false,
        subscriber_count: 0,
    }
}

/// Builds a fake [`rusty_ytdl::Embed`] for testing purposes.
#[must_use]
pub fn build_mock_rusty_embed() -> rusty_ytdl::Embed {
    rusty_ytdl::Embed {
        flash_secure_url: "flash_secure_url".to_string(),
        flash_url: "flash_url".to_string(),
        iframe_url: "iframe_url".to_string(),
        width: 0,
        height: 0,
    }
}

/// Builds a mock [`VideoDetails`] for testing purposes.
#[must_use]
pub fn build_mock_rusty_video_details() -> rusty_ytdl::VideoDetails {
    rusty_ytdl::VideoDetails {
        author: Some(build_mock_rusty_author()),
        likes: 0,
        dislikes: 0,
        age_restricted: false,
        video_url: "https://www.youtube.com/watch?v=meta123".to_string(),
        storyboards: vec![],
        chapters: vec![],
        embed: build_mock_rusty_embed(),
        title: "Title".to_string(),
        description: "description".to_string(),
        length_seconds: "60".to_string(),
        owner_profile_url: "owner_profile_url".to_string(),
        external_channel_id: "external_channel_id".to_string(),
        is_family_safe: false,
        available_countries: vec![],
        is_unlisted: false,
        has_ypc_metadata: false,
        view_count: "0".to_string(),
        category: "category".to_string(),
        publish_date: "publish_date".to_string(),
        owner_channel_name: "owner_channel_name".to_string(),
        upload_date: "upload_date".to_string(),
        video_id: "meta123".to_string(),
        keywords: vec![],
        channel_id: "channel_id".to_string(),
        is_owner_viewing: false,
        is_crawlable: false,
        allow_ratings: false,
        is_private: false,
        is_unplugged_corpus: false,
        is_live_content: false,
        thumbnails: build_mock_thumbnails(),
    }
}

/// Builds a fake but valid [`Token`] for testing purposes.
/// # Panics
/// * If the token is invalid.
#[must_use]
pub fn get_valid_token() -> Token {
    //validate(DEFAULT_VALID_TOKEN).expect("Invalid token");
    crate::DEFAULT_VALID_TOKEN_TOKEN.clone()
}
