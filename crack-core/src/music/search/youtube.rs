//! YouTube as a search source: rusty_ytdl's search (one request, metadata
//! included), and yt-dlp's `ytsearch` when rusty_ytdl errors.

use super::{
    gated, saved_from_metadata, Hit, HitDetail, Query, SourceResult, EXTERNAL, HITS_PER_SOURCE,
};
use crate::music::MusicData;
use songbird::input::AuxMetadata;

/// How many hits the yt-dlp fallback asks for.
pub(crate) const YTDLP_HITS: usize = 5;

pub(crate) async fn search_youtube(data: &MusicData, q: &Query) -> SourceResult {
    gated(&EXTERNAL, async {
        let rusty = data
            .ct_client
            .search_metadata(q.as_str(), HITS_PER_SOURCE as u64)
            .await;
        if let Some(done) = from_rusty(rusty) {
            return done;
        }
        match crack_types::ytdl_search(crack_types::Site::YouTube, q.as_str(), YTDLP_HITS).await {
            Ok(metas) => found(metas),
            Err(e) => {
                tracing::warn!("search: yt-dlp YouTube search failed: {e}");
                SourceResult::Failed
            },
        }
    })
    .await
}

/// What rusty_ytdl's answer amounts to: `Some` to use it, `None` to ask
/// yt-dlp instead. rusty_ytdl returns `Ok(vec![])` when YouTube's markup
/// changes (it skips shapes it does not recognise), so an answer with no
/// usable hit is a failure too, not "no matches".
pub(crate) fn from_rusty<E: std::fmt::Display>(
    rusty: Result<Vec<AuxMetadata>, E>,
) -> Option<SourceResult> {
    match rusty {
        Ok(metas) => {
            let hits = found(metas);
            if matches!(&hits, SourceResult::Hits(h) if h.is_empty()) {
                tracing::warn!("search: rusty_ytdl found nothing usable, asking yt-dlp");
                None
            } else {
                Some(hits)
            }
        },
        Err(e) => {
            tracing::warn!("search: rusty_ytdl failed, asking yt-dlp: {e}");
            None
        },
    }
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

    #[test]
    fn rusty_ytdl_answers_are_used_unless_empty_or_failed() {
        let used = from_rusty::<String>(Ok(vec![m(Some("https://y/a"))]));
        assert!(matches!(used, Some(SourceResult::Hits(h)) if h.len() == 1));
        assert!(from_rusty::<String>(Ok(vec![])).is_none(), "Ok(empty)");
        assert!(
            from_rusty::<String>(Ok(vec![m(None)])).is_none(),
            "nothing with a web URL"
        );
        assert!(from_rusty::<String>(Err("boom".into())).is_none(), "Err");
    }

    #[tokio::test]
    #[ignore = "hits live YouTube"]
    async fn a_live_youtube_search_finds_something() {
        let data = crate::music::ops::test_support::test_music_data();
        let SourceResult::Hits(h) =
            search_youtube(&data, &Query::new("abba waterloo").unwrap()).await
        else {
            panic!()
        };
        assert!(!h.is_empty());
    }
}
