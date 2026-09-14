//! YouTube's own Mix for the track that just ended, listed through yt-dlp.
//!
//! Every video has an auto-generated Mix playlist, `watch?v=<id>&list=RD<id>`,
//! and `yt-dlp --flat-playlist -J` lists it. Measured 2026-09-13: 1-2 seconds,
//! no key, no quota and no seed. For a fan upload of The Offspring it answered
//! blink-182, Sum 41, Linkin Park and System Of A Down, where ReccoBeats'
//! audio-feature matching answered Calvin Harris and Banda Toro. Its first
//! entry is the video itself.
//!
//! yt-dlp is already the bot's playback dependency and must be on PATH in the
//! container, so this adds no new one.

use super::http;
use crate::{Error, Playable, RawTrack, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashSet;
use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;

const NAME: &str = "youtube-mix";

/// How long one listing may take. Measured at 1-2 seconds; a yt-dlp that hangs
/// must not hold a track end for ever.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// 🪤 `entries` is not defaulted. yt-dlp prints `null` for a video it cannot
/// open, and a page that is not a playlist at all must not read as an empty
/// Mix. `channel` and `uploader` are genuinely optional per entry.
#[derive(Debug, Deserialize)]
struct Playlist {
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    id: String,
    title: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
}

/// A YouTube video id: exactly 11 of `[A-Za-z0-9_-]`.
///
/// The id is interpolated into the Mix URL handed to yt-dlp. It is one argv
/// entry and never passes through a shell, but an id carrying `&` or `#` would
/// still rewrite that URL.
#[must_use]
pub fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The video id in a YouTube URL: `watch?v=`, `youtu.be/`, `shorts/`, `live/`
/// and `embed/`, on `youtube.com`, `www.`, `m.` and `music.youtube.com`, and
/// `youtu.be`. `None` for anything else, including an id that is not 11 of
/// `[A-Za-z0-9_-]`.
#[must_use]
pub fn video_id_from_url(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let id = match url.host_str()? {
        "youtu.be" => url.path_segments()?.next().map(str::to_owned),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            let mut segments = url.path_segments()?;
            match segments.next() {
                Some("watch") => url
                    .query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, value)| value.into_owned()),
                Some("shorts" | "live" | "embed") => segments.next().map(str::to_owned),
                _ => None,
            }
        },
        _ => None,
    }?;
    is_video_id(&id).then_some(id)
}

/// YouTube's Mix, through yt-dlp.
#[derive(Debug, Clone)]
pub struct YouTubeMix {
    program: OsString,
    timeout: Duration,
}

impl Default for YouTubeMix {
    fn default() -> Self {
        Self::new()
    }
}

impl YouTubeMix {
    /// yt-dlp from PATH.
    #[must_use]
    pub fn new() -> Self {
        Self::with_program("yt-dlp")
    }

    /// A different yt-dlp. Tests point this at a stand-in script.
    #[must_use]
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            timeout: TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn args(video_id: &str, want: usize) -> Vec<String> {
        vec![
            "--flat-playlist".into(),
            "--no-warnings".into(),
            "-J".into(),
            "--playlist-items".into(),
            // One extra: the Mix opens with the video itself.
            format!("1:{}", want + 1),
            format!("https://www.youtube.com/watch?v={video_id}&list=RD{video_id}"),
        ]
    }
}

#[async_trait]
impl crate::provider::Recommender for YouTubeMix {
    fn name(&self) -> &'static str {
        NAME
    }

    fn needs_seed(&self) -> bool {
        false
    }

    async fn recommend(
        &self,
        track: &RawTrack,
        _seed: Option<&Seed>,
        want: usize,
    ) -> Result<Vec<Recommendation>> {
        let Some(video_id) = track.video_id.as_deref().filter(|id| is_video_id(id)) else {
            return Ok(Vec::new());
        };
        if want == 0 {
            return Ok(Vec::new());
        }

        let child = tokio::process::Command::new(&self.program)
            .args(Self::args(video_id, want))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A listing that times out is dropped below; this makes dropping
            // it kill yt-dlp instead of leaving it running.
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Config(format!("{NAME}: could not run {:?}: {e}", self.program)))?;
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| Error::UnexpectedBody {
                provider: NAME,
                message: format!("yt-dlp gave no answer within {:?}", self.timeout),
            })?
            .map_err(|e| Error::UnexpectedBody {
                provider: NAME,
                message: format!("reading yt-dlp's output: {e}"),
            })?;
        if !output.status.success() {
            return Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!(
                    "yt-dlp exited with {}: {}",
                    output.status,
                    http::excerpt(String::from_utf8_lossy(&output.stderr).trim())
                ),
            });
        }
        let playlist: Playlist =
            serde_json::from_slice(&output.stdout).map_err(|e| Error::UnexpectedBody {
                provider: NAME,
                message: format!(
                    "{e}: {}",
                    http::excerpt(&String::from_utf8_lossy(&output.stdout))
                ),
            })?;

        // The video itself is seeded into `seen`, so it is dropped wherever
        // it appears, along with any repeat.
        let mut seen = HashSet::from([video_id.to_owned()]);
        Ok(playlist
            .entries
            .into_iter()
            .filter(|e| is_video_id(&e.id) && seen.insert(e.id.clone()))
            .map(|e| Recommendation {
                // The uploading channel: for a fan upload that is not the
                // artist. It only labels the result, which plays by its id.
                artist: e.channel.or(e.uploader).unwrap_or_default(),
                title: e.title,
                playable: Playable::YouTubeId(e.id),
                isrc: None,
                source: NAME.into(),
            })
            .take(want)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_ids_come_out_of_every_youtube_url_shape() {
        for (url, want) in [
            (
                "https://www.youtube.com/watch?v=NJKhbnSGLsQ",
                Some("NJKhbnSGLsQ"),
            ),
            (
                "https://www.youtube.com/watch?list=RDx&v=NJKhbnSGLsQ&t=42",
                Some("NJKhbnSGLsQ"),
            ),
            ("https://youtu.be/HwRL1LNVTLI?si=abc", Some("HwRL1LNVTLI")),
            (
                "https://music.youtube.com/watch?v=5_PBNZXlzp0",
                Some("5_PBNZXlzp0"),
            ),
            (
                "https://m.youtube.com/watch?v=-bcdefghijk",
                Some("-bcdefghijk"),
            ),
            (
                "https://youtube.com/shorts/NJKhbnSGLsQ",
                Some("NJKhbnSGLsQ"),
            ),
            (
                "https://www.youtube.com/live/NJKhbnSGLsQ",
                Some("NJKhbnSGLsQ"),
            ),
            (
                "https://www.youtube.com/embed/NJKhbnSGLsQ",
                Some("NJKhbnSGLsQ"),
            ),
            ("https://www.youtube.com/playlist?list=PL123", None),
            ("https://www.youtube.com/watch?v=tooshort", None),
            // Decodes to "NJKhbnSGLsQ&x": an id that would rewrite the Mix URL.
            ("https://www.youtube.com/watch?v=NJKhbnSGLsQ%26x", None),
            (
                "https://open.spotify.com/track/3lfmqF0ULXRHlWxBeaHo3t",
                None,
            ),
            ("https://notyoutube.com/watch?v=NJKhbnSGLsQ", None),
            ("not a url", None),
        ] {
            assert_eq!(video_id_from_url(url).as_deref(), want, "{url}");
        }
    }

    #[test]
    fn a_video_id_is_eleven_url_safe_characters() {
        assert!(is_video_id("NJKhbnSGLsQ"));
        assert!(is_video_id("-_bcdefghij"));
        assert!(!is_video_id("NJKhbnSGLs"), "10");
        assert!(!is_video_id("NJKhbnSGLsQQ"), "12");
        assert!(!is_video_id("NJKhbnSG&sQ"));
    }

    #[test]
    fn it_needs_no_seed() {
        use crate::provider::Recommender as _;
        assert!(!YouTubeMix::new().needs_seed());
    }

    #[cfg(unix)]
    mod with_a_stand_in {
        use super::super::*;
        use crate::provider::Recommender as _;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        /// A stand-in for yt-dlp that records its arguments one per line, then
        /// runs `body`. One directory per test, so tests running in parallel
        /// never share a script.
        fn stand_in(test: &str, body: &str) -> (PathBuf, PathBuf) {
            let dir = std::env::temp_dir().join(format!(
                "musicreco-youtube-mix-{}-{test}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let args = dir.join("args");
            let _ = std::fs::remove_file(&args);
            let program = dir.join("yt-dlp");
            std::fs::write(
                &program,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n{body}\n",
                    args.display()
                ),
            )
            .expect("write the stand-in");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
                .expect("make it executable");
            (program, args)
        }

        fn prints(json: &str) -> String {
            format!("cat <<'JSON'\n{json}\nJSON")
        }

        /// 🪤 Every test here that writes or runs a stand-in holds this. Linux
        /// refuses to exec a file that a concurrent fork still holds open for
        /// writing: measured, 4 full runs in 15 failed with "Text file busy
        /// (os error 26)" while these ran in parallel.
        async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
            static SERIAL: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
            SERIAL
                .get_or_init(|| tokio::sync::Mutex::new(()))
                .lock()
                .await
        }

        /// The shape yt-dlp 2026.08.19 printed for this Mix, trimmed: the video
        /// itself first, a repeat, and one entry whose `channel` is null.
        const MIX: &str = r#"{"_type":"playlist","id":"RDNJKhbnSGLsQ","title":"Mix - The Offspring ~ Hit That","entries":[
{"_type":"url","ie_key":"Youtube","id":"NJKhbnSGLsQ","title":"The Offspring ~ Hit That","channel":"MrCalienteLP","uploader":"MrCalienteLP","duration":169},
{"_type":"url","ie_key":"Youtube","id":"ttPFawRGCQw","title":"The Offspring ~ Want You Bad","channel":"MrCalienteLP","uploader":"MrCalienteLP","duration":203},
{"_type":"url","ie_key":"Youtube","id":"4QWzYjatCCs","title":"Jimmy Eat World- The Middle HQ","channel":null,"uploader":"Humberto Lopez","duration":166},
{"_type":"url","ie_key":"Youtube","id":"ttPFawRGCQw","title":"The Offspring ~ Want You Bad","channel":"MrCalienteLP","uploader":"MrCalienteLP","duration":203},
{"_type":"url","ie_key":"Youtube","id":"5_PBNZXlzp0","title":"All The Small Things","channel":"blink-182","uploader":"blink-182","duration":168}]}"#;

        fn ended(video_id: Option<&str>) -> RawTrack {
            RawTrack {
                title: "The Offspring ~ Hit That".into(),
                artist: None,
                uploader: Some("MrCalienteLP".into()),
                video_id: video_id.map(str::to_owned),
            }
        }

        #[tokio::test]
        async fn the_mix_plays_by_id_without_the_video_itself_or_repeats() {
            let _serial = serial().await;
            let (program, _) = stand_in("mix", &prints(MIX));
            let out = YouTubeMix::with_program(program)
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .unwrap();
            let ids: Vec<_> = out.iter().map(|r| r.youtube_id().unwrap()).collect();
            assert_eq!(ids, ["ttPFawRGCQw", "4QWzYjatCCs", "5_PBNZXlzp0"]);
            assert_eq!(out[0].title, "The Offspring ~ Want You Bad");
            assert_eq!(out[0].source, "youtube-mix");
            assert_eq!(
                out[1].artist, "Humberto Lopez",
                "a null channel falls back to the uploader"
            );
            assert_eq!(out[2].artist, "blink-182");
        }

        /// Asserted on what yt-dlp was actually given: every response-shaped
        /// assertion above would pass with the wrong URL or a missing flag.
        #[tokio::test]
        async fn yt_dlp_is_asked_for_the_mix_with_one_entry_to_spare() {
            let _serial = serial().await;
            let (program, args) = stand_in("args", &prints(MIX));
            YouTubeMix::with_program(program)
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(&args).expect("yt-dlp was run"),
                "--flat-playlist\n--no-warnings\n-J\n--playlist-items\n1:6\n\
                 https://www.youtube.com/watch?v=NJKhbnSGLsQ&list=RDNJKhbnSGLsQ\n"
            );
        }

        #[tokio::test]
        async fn want_is_a_ceiling() {
            let _serial = serial().await;
            let (program, _) = stand_in("want", &prints(MIX));
            let out = YouTubeMix::with_program(program)
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 2)
                .await
                .unwrap();
            assert_eq!(out.len(), 2);
        }

        #[tokio::test]
        async fn no_usable_video_id_runs_nothing() {
            let _serial = serial().await;
            let (program, args) = stand_in("no-id", &prints(MIX));
            let mix = YouTubeMix::with_program(program);
            for id in [None, Some("not-an-id"), Some("NJKhbnSG&sQ")] {
                assert!(mix.recommend(&ended(id), None, 5).await.unwrap().is_empty());
            }
            assert!(
                mix.recommend(&ended(Some("NJKhbnSGLsQ")), None, 0)
                    .await
                    .unwrap()
                    .is_empty(),
                "want 0"
            );
            assert!(!args.exists(), "yt-dlp must not have been run at all");
        }

        /// Measured: `ERROR: [youtube] AAAAAAAAAAA: This video is unavailable`
        /// on stderr, `null` on stdout, exit 1.
        #[tokio::test]
        async fn a_failing_yt_dlp_is_an_error_carrying_its_stderr() {
            let _serial = serial().await;
            let (program, _) = stand_in(
                "fails",
                "echo 'ERROR: [youtube] NJKhbnSGLsQ: This video is unavailable' >&2\necho null\nexit 1",
            );
            let err = YouTubeMix::with_program(program)
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .expect_err("exit 1");
            match err {
                Error::UnexpectedBody { message, .. } => {
                    assert!(message.contains("This video is unavailable"), "{message}")
                },
                other => panic!("expected UnexpectedBody, got {other}"),
            }
        }

        #[tokio::test]
        async fn output_that_is_not_a_playlist_is_an_error_not_an_empty_mix() {
            let _serial = serial().await;
            let (program, _) = stand_in("not-a-mix", &prints(r#"{"_type":"video","id":"x"}"#));
            let err = YouTubeMix::with_program(program)
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .expect_err("no entries field");
            assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
        }

        #[tokio::test]
        async fn a_missing_yt_dlp_is_a_configuration_error() {
            let _serial = serial().await;
            let err = YouTubeMix::with_program("/nonexistent/musicreco/yt-dlp")
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .expect_err("cannot spawn");
            assert!(matches!(err, Error::Config(_)), "got {err}");
        }

        #[tokio::test]
        async fn a_yt_dlp_that_hangs_is_given_up_on() {
            let _serial = serial().await;
            let (program, _) = stand_in("hangs", "sleep 5");
            let started = std::time::Instant::now();
            let err = YouTubeMix::with_program(program)
                .with_timeout(Duration::from_millis(200))
                .recommend(&ended(Some("NJKhbnSGLsQ")), None, 5)
                .await
                .expect_err("timed out");
            assert!(
                started.elapsed() < Duration::from_secs(4),
                "did not wait out the sleep"
            );
            match err {
                Error::UnexpectedBody { message, .. } => {
                    assert!(message.contains("no answer within"), "{message}")
                },
                other => panic!("expected UnexpectedBody, got {other}"),
            }
        }
    }
}
