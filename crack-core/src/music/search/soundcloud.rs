//! SoundCloud as a search source: yt-dlp's `scsearch`, through the one
//! sanctioned search constructor.

use super::{gated, youtube::found, Query, SourceResult, EXTERNAL};

/// SoundCloud asks for five, as the YouTube fallback does.
pub(crate) const SOUNDCLOUD_HITS: usize = 5;

pub(crate) async fn search_soundcloud(q: &Query) -> SourceResult {
    gated(&EXTERNAL, async {
        match crack_types::ytdl_search(crack_types::Site::SoundCloud, q.as_str(), SOUNDCLOUD_HITS)
            .await
        {
            Ok(metas) => found(metas),
            Err(e) => {
                tracing::warn!("search: yt-dlp SoundCloud search failed: {e}");
                SourceResult::Failed
            },
        }
    })
    .await
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    #[ignore = "hits live SoundCloud and needs yt-dlp on PATH"]
    async fn a_live_soundcloud_search_finds_soundcloud_links() {
        let SourceResult::Hits(h) = search_soundcloud(&Query::new("lofi").unwrap()).await else {
            panic!()
        };
        assert!(!h.is_empty() && h.iter().all(|x| x.track.url.contains("soundcloud.com")));
    }
}
