use crate::errors::CrackedError;
use crate::guild::settings::VIDEO_WATCH_URL;
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
                    let url = format!("{}{}", VIDEO_WATCH_URL, &id_string);
                    drop(id_string);
                    Some(url)
                }
            })
            .collect::<Vec<String>>())
    }
}

#[cfg(test)]
mod test {

    #[tokio::test]
    async fn test_ytdl() {
        let url =
            "https://www.youtube.com/playlist?list=PLzk-s3QLDrQ8tGpRzZ01woRoUd4ed-84q".to_string();
        let mut ytdl = crate::sources::ytdl::MyYoutubeDl::new(url);
        let playlist = ytdl.get_playlist().await;
        if playlist.is_err() {
            println!(
                "{:?}",
                playlist
                    .unwrap_err()
                    .to_string()
                    .contains("Your IP is likely blocked by YouTube.")
            );
        } else {
            println!("{:?}", playlist.unwrap());
        }
    }
}
