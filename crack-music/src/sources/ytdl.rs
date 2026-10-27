use crack_common::errors::CrackedError;
use crack_guild::settings::VIDEO_WATCH_URL;
use std::fmt::Display;
use tokio::process::Command;
use tokio::runtime::Handle;

use once_cell::sync::Lazy; // 1.5.2
pub static HANDLE: Lazy<std::sync::Mutex<Option<Handle>>> = Lazy::new(Default::default);
const YOUTUBE_DL_COMMAND: &str = "yt-dlp";

#[derive(Clone, Debug)]
enum QueryType {
    Url(String),
    //Search(String),
}

impl Display for QueryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryType::Url(url) => write!(f, "{}", url),
            //QueryType::Search(query) => write!(f, "ytsearch5:{}", query),
        }
    }
}

/// A lazily instantiated call to download a file, finding its URL via youtube-dl.
///
/// By default, this uses yt-dlp and is backed by an [`HttpRequest`]. This handler
/// attempts to find the best audio-only source (typically `WebM`, enabling low-cost
/// Opus frame passthrough).
///
/// [`HttpRequest`]: songbird::input::HttpRequest
#[derive(Clone, Debug)]
pub struct MyYoutubeDl {
    program: &'static str,
    query: QueryType,
}

impl MyYoutubeDl {
    /// Creates a lazy request to select an audio stream from `url`, using "yt-dlp".
    ///
    /// This requires a reqwest client: ideally, one should be created and shared between
    /// all requests.
    #[must_use]
    pub fn new(url: String) -> Self {
        Self::new_ytdl_like(YOUTUBE_DL_COMMAND, url)
    }

    /// Creates a lazy request to select an audio stream from `url` as in [`new`], using `program`.
    ///
    /// [`new`]: Self::new
    #[must_use]
    pub fn new_ytdl_like(program: &'static str, url: String) -> Self {
        Self {
            program,
            query: QueryType::Url(url),
        }
    }

    /// Gets all the URLs in a YouTube playlist.
    pub async fn get_playlist(&mut self) -> Result<Vec<String>, CrackedError> {
        let ytdl_args = [
            // "-j",
            "--flat-playlist",
            //"--get-title",
            "--get-id",
            // 🔒 Everything after `--` is a URL, never an option.
            "--",
            &self.query.to_string(),
        ];
        let output = Command::new(self.program).args(ytdl_args).output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(CrackedError::CommandFailed(
                self.program,
                output.status,
                stderr.into(),
            ));
        }
        Ok(output
            .stdout
            .split(|&b| b == b'\n')
            .filter_map(|x| {
                if x.is_empty() {
                    None
                } else {
                    let id_string = String::from_utf8_lossy(x);
                    let url = format!("{}{}", VIDEO_WATCH_URL, id_string);
                    drop(id_string);
                    Some(url)
                }
            })
            .collect::<Vec<String>>())
    }
}

#[cfg(test)]
mod test {
    /// The production failure, end to end: yt-dlp picks SoundCloud's format,
    /// songbird fetches it, and symphonia probes it. Without the format sort
    /// this fails with "adts: only 1 aac frame per adts packet is supported".
    /// It lives in crack-core because crack-core's symphonia has the readers
    /// the bot ships with; crack-types' own test binary has no MP3 reader.
    #[tokio::test]
    #[ignore = "hits live SoundCloud and needs yt-dlp on PATH"]
    async fn a_soundcloud_track_is_playable() {
        use songbird::input::{
            codecs::{get_codec_registry, get_probe},
            Input,
        };
        let ytdl = crack_types::ytdl_for_url(
            reqwest::Client::new(),
            "https://soundcloud.com/realtimechris/t-sne-the-whole-thing-into-oblivion",
        )
        .unwrap();
        let input: Input = ytdl.into();
        let playable = input
            .make_playable_async(get_codec_registry(), get_probe())
            .await;
        assert!(playable.is_ok(), "{:?}", playable.err());
    }

    /// YouTube refuses a stream URL until yt-dlp's `available_at`, when any
    /// pre-roll ad served with it would have ended, and on 2026-10-27 that
    /// failed every track with 403. The CycleFive songbird waits for it before
    /// fetching; this stops compiling if the workspace goes back to a songbird
    /// that doesn't.
    #[test]
    fn songbird_waits_for_available_at() {
        use std::time::{Duration, UNIX_EPOCH};

        let output: songbird::input::metadata::YoutubeDlOutput =
            serde_json::from_str(r#"{"url":"u","available_at":1005}"#).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1000);
        assert_eq!(
            output.time_until_available(now),
            Some(Duration::from_secs(5))
        );
    }

    /// Two unskippable pre-roll ads can hold a URL for 30 to 40 s, longer
    /// than the 30 s cap the fork started with; one cut short is refused.
    #[test]
    fn songbird_waits_out_a_forty_second_ad_break() {
        use std::time::{Duration, UNIX_EPOCH};

        let output: songbird::input::metadata::YoutubeDlOutput =
            serde_json::from_str(r#"{"url":"u","available_at":1040}"#).unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1000);
        assert_eq!(
            output.time_until_available(now),
            Some(Duration::from_secs(40))
        );
    }

    /// A stand-in for yt-dlp that prints one result, pointing at a port where
    /// nothing listens, and counts how often it runs.
    #[cfg(unix)]
    struct FakeYtdl {
        program: String,
    }

    #[cfg(unix)]
    impl FakeYtdl {
        fn new(name: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;

            let program = std::env::temp_dir()
                .join(format!("crack-fake-ytdl-{}-{name}", std::process::id()))
                .to_str()
                .unwrap()
                .to_owned();
            std::fs::write(
                &program,
                "#!/bin/sh\necho run >> \"$0.runs\"\necho '{\"url\":\"http://127.0.0.1:9/\",\"title\":\"t\"}'\n",
            )
            .unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            let _ = std::fs::remove_file(format!("{program}.runs"));
            Self { program }
        }

        fn runs(&self) -> usize {
            std::fs::read_to_string(format!("{}.runs", self.program))
                .map_or(0, |runs| runs.lines().count())
        }

        #[expect(
            clippy::disallowed_methods,
            reason = "runs the stand-in; the URL is a fixed word, never user text"
        )]
        fn youtubedl(&self) -> songbird::input::YoutubeDl<'_> {
            songbird::input::YoutubeDl::new_ytdl_like(
                &self.program,
                reqwest::Client::new(),
                "unused",
            )
        }
    }

    #[cfg(unix)]
    impl Drop for FakeYtdl {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.program);
            let _ = std::fs::remove_file(format!("{}.runs", self.program));
        }
    }

    /// `query.rs` fetches a track's metadata and then plays the same
    /// `YoutubeDl`, which ran yt-dlp twice until the CycleFive songbird kept
    /// the first resolve. Fails if the workspace goes back to a songbird that
    /// resolves again to open the stream.
    #[cfg(unix)]
    #[tokio::test]
    async fn songbird_opens_the_stream_from_the_metadata_resolve() {
        use songbird::input::Compose;

        let fake = FakeYtdl::new("reuse");
        let mut ytdl = fake.youtubedl();
        ytdl.aux_metadata().await.unwrap();
        assert!(ytdl.create_async().await.is_err());
        assert_eq!(fake.runs(), 1, "opening the stream ran yt-dlp again");
    }

    /// `/play` logs its query with `{:?}`, and a link's query holds the
    /// resolved `YoutubeDl`, whose kept resolve has a signed stream URL that
    /// carries the bot's IP address.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_resolved_youtubedl_keeps_its_stream_url_out_of_logs() {
        use songbird::input::Compose;

        let fake = FakeYtdl::new("debug");
        let mut ytdl = fake.youtubedl();
        ytdl.aux_metadata().await.unwrap();
        assert!(!format!("{ytdl:?}").contains("127.0.0.1:9"));
    }
}
