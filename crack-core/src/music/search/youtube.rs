//! YouTube as a search source: rusty_ytdl's search (one request, metadata
//! included), and yt-dlp's `ytsearch` when rusty_ytdl errors.

use super::{
    gated, saved_from_metadata, Hit, HitDetail, Query, SourceResult, EXTERNAL, HITS_PER_SOURCE,
};
use crate::Data;
use songbird::input::AuxMetadata;

/// How many hits the yt-dlp fallback asks for: it extracts each one in full.
pub(crate) const YTDLP_HITS: usize = 5;

pub(crate) async fn search_youtube(data: &Data, q: &Query) -> SourceResult {
    gated(&EXTERNAL, async {
        match data
            .ct_client
            .search_metadata(q.as_str(), HITS_PER_SOURCE as u64)
            .await
        {
            Ok(metas) => found(metas),
            Err(e) => {
                tracing::warn!("search: rusty_ytdl failed, asking yt-dlp: {e}");
                let mut ytdl = crack_types::ytdl_search(
                    crate::http_utils::get_client_old().clone(),
                    crack_types::Site::YouTube,
                    q.as_str(),
                    YTDLP_HITS,
                );
                match ytdl.search(Some(YTDLP_HITS)).await {
                    Ok(metas) => found(metas.collect()),
                    Err(e) => {
                        tracing::warn!("search: yt-dlp YouTube search failed: {e}");
                        SourceResult::Failed
                    },
                }
            },
        }
    })
    .await
}

/// Search metadata as `Found` hits, in order, web URLs only, capped.
pub(crate) fn found(metas: Vec<AuxMetadata>) -> SourceResult {
    SourceResult::Hits(
        metas
            .iter()
            .filter_map(saved_from_metadata)
            .take(HITS_PER_SOURCE)
            .map(|track| Hit {
                track,
                detail: HitDetail::Found,
            })
            .collect(),
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use songbird::input::AuxMetadata;

    fn m(url: Option<&str>) -> AuxMetadata {
        AuxMetadata {
            title: Some("t".into()),
            source_url: url.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn found_hits_keep_order_drop_non_web_and_cap() {
        let mut metas = vec![m(Some("https://y/a")), m(None), m(Some("https://y/b"))];
        metas.extend((0..20).map(|i| m(Some(&format!("https://y/{i}")))));
        let SourceResult::Hits(hits) = found(metas) else {
            panic!()
        };
        assert_eq!(hits.len(), HITS_PER_SOURCE);
        assert_eq!(
            (hits[0].track.url.as_str(), hits[1].track.url.as_str()),
            ("https://y/a", "https://y/b")
        );
        assert!(hits.iter().all(|h| h.detail == HitDetail::Found));
    }

    #[tokio::test]
    #[ignore = "hits live YouTube"]
    async fn a_live_youtube_search_finds_something() {
        let data = crate::Data::default();
        let SourceResult::Hits(h) =
            search_youtube(&data, &Query::new("abba waterloo").unwrap()).await
        else {
            panic!()
        };
        assert!(!h.is_empty());
    }
}
