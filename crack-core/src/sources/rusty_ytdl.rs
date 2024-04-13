use crate::errors::CrackedError;
use rusty_ytdl::search::{SearchOptions, SearchResult, YouTube};
// use std::fmt::Display;

/// Out strucut to wrap the rusty-ytdl search instance
//TODO expand to go beyond search
#[derive(Clone, Debug)]
pub struct MyRustyYoutubeDl {
    rusty_ytdl: YouTube,
}

/// Implementation of the `MyRustyYoutubeDl` struct.
impl MyRustyYoutubeDl {
    /// Creates a new instance of `MyRustyYoutubeDl`. Optionally takes a `YouTube` instance.
    pub fn new(rytdlp: Option<YouTube>) -> Result<Self, CrackedError> {
        Ok(Self {
            rusty_ytdl: rytdlp.unwrap_or(YouTube::new()?),
        })
    }

    // Do a one-shot search
    pub async fn one_shot(&mut self, query: String) -> Result<Vec<SearchResult>, CrackedError> {
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

    #[tokio::test]
    async fn test_ytdl() {
        // let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let mut ytdl = crate::sources::rusty_ytdl::MyRustyYoutubeDl::new(None).unwrap();
        let playlist = ytdl
            .one_shot("The Night Chicago Died".to_string())
            .await
            .unwrap();
        println!("{:?}", playlist);
    }
}
