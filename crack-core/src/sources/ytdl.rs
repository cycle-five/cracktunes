use crate::errors::CrackedError;
use reqwest::Client;
use songbird::input::AuxMetadata;
use std::fmt::Display;
use tokio::process::Command;

const YOUTUBE_DL_COMMAND: &str = "yt-dlp";

#[derive(Clone, Debug)]
enum QueryType {
    Url(String),
    Search(String),
}

impl Display for QueryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryType::Url(url) => write!(f, "{}", url),
            QueryType::Search(query) => write!(f, "ytsearch5:{}", query),
        }
    }
}

/// A lazily instantiated call to download a file, finding its URL via youtube-dl.
///
/// By default, this uses yt-dlp and is backed by an [`HttpRequest`]. This handler
/// attempts to find the best audio-only source (typically `WebM`, enabling low-cost
/// Opus frame passthrough).
///
/// [`HttpRequest`]: super::HttpRequest
#[derive(Clone, Debug)]
pub struct MyYoutubeDl {
    program: &'static str,
    client: Client,
    metadata: Option<AuxMetadata>,
    query: QueryType,
}

impl MyYoutubeDl {
    /// Creates a lazy request to select an audio stream from `url`, using "yt-dlp".
    ///
    /// This requires a reqwest client: ideally, one should be created and shared between
    /// all requests.
    #[must_use]
    pub fn new(client: Client, url: String) -> Self {
        Self::new_ytdl_like(YOUTUBE_DL_COMMAND, client, url)
    }

    /// Creates a lazy request to select an audio stream from `url` as in [`new`], using `program`.
    ///
    /// [`new`]: Self::new
    #[must_use]
    pub fn new_ytdl_like(program: &'static str, client: Client, url: String) -> Self {
        Self {
            program,
            client,
            metadata: None,
            query: QueryType::Url(url),
        }
    }

    async fn get_playlist(&mut self) -> Result<Vec<String>, CrackedError> {
        let ytdl_args = [
            "-j",
            "--flat-playlist",
            "--get-title",
            "--get-id",
            &self.query.to_string(),
        ];
        let output = Command::new(self.program)
            .args(ytdl_args)
            .output()
            .await
            .map_err(|e| CrackedError::IO(e.into()))?;

        if !output.status.success() {
            return Err(CrackedError::CommandFailed(
                self.program.to_string(),
                output.status,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        Ok(output
            .stdout
            .split(|&b| b == b'\n')
            .filter_map(|x| {
                serde_json::from_slice(x)
                    .ok()
                    .map(|x: serde_json::Value| x.as_str().unwrap().to_string())
            })
            .collect::<Vec<String>>())
    }
}

mod test {
    use super::*;
    use crate::sources::ytdl::MyYoutubeDl;
    use reqwest::Client;
    use songbird::input::Input;
    use tokio::runtime::Runtime;

    #[tokio::test]
    async fn test_ytdl() {
        let client = Client::new();
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let mut ytdl = MyYoutubeDl::new(client, url);
        let playlist = ytdl.get_playlist().await?;
        println!("{:?}", playlist);
    }
}
