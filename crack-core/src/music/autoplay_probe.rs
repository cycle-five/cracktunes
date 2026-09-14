//! A local reproduction of autoplay's recommendation path, against the live
//! services. It runs what a track end runs, minus Discord and the database:
//! the query's metadata exactly as `/play` attaches it (`ct_client` first, the
//! `ready_query` fallback only if that fails), `raw_track`, the seed
//! each resolver would produce, then `MusicReco::next_tracks` as
//! `build_musicreco` wires it in production (no musicatlas key, no pool).
//!
//! ```text
//! AUTOPLAY_PROBE='https://www.youtube.com/watch?v=NJKhbnSGLsQ;hit that' \
//!   SQLX_OFFLINE=true cargo test -p crack-core --lib autoplay_probe -- --ignored --nocapture
//! ```
//!
//! Queries are separated by `;`. A query starting with `http` is a video link,
//! anything else is keywords. `RUST_LOG` overrides the default log filter.
//! `AUTOPLAY_PROBE_NO_VIDEO_ID=1` drops the video id, so YouTube's Mix has
//! nothing and the fallbacks answer.

use crate::music::autoplay::{build_musicreco, raw_track, to_query, REFILL_SIZE};
use crate::music::query::NewQueryType;
use crack_musicreco::{MusicBrainz, SeedResolver, TitleParseResolver};
use crack_testing::CrackTrackClient;
use crack_types::QueryType;
use std::time::Duration;

#[tokio::test]
#[ignore = "hits YouTube, MusicBrainz and Deezer; run by hand with AUTOPLAY_PROBE"]
async fn autoplay_probe() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "warn,crack_musicreco=debug,crack_core::music::autoplay=debug",
        )
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_test_writer()
        .try_init();

    let queries = std::env::var("AUTOPLAY_PROBE")
        .expect("set AUTOPLAY_PROBE to a video URL or keywords (several separated by ';')");
    let reco = build_musicreco(None, None).expect("YouTube's Mix alone always builds");
    println!("recommenders: {:?}", reco.recommender_names());

    for query in queries.split(';').map(str::trim).filter(|q| !q.is_empty()) {
        println!("\n=== {query}");
        let qt = if query.starts_with("http") {
            QueryType::VideoLink(query.to_string())
        } else {
            QueryType::Keywords(query.to_string())
        };
        // The same order as `queue_track_back`: `ct_client` first, whose
        // `ResolvedTrack.metadata` is what `build_track` puts on the track, and
        // `ready_query`'s `get_track_source_and_metadata` only if that fails.
        let meta = match CrackTrackClient::new().resolve_track(qt.clone()).await {
            Ok(resolved) => {
                println!("path: ct_client.resolve_track");
                resolved.metadata
            },
            Err(e) => {
                println!("path: ct_client failed ({e}); falling back to ready_query");
                match NewQueryType(qt).get_track_source_and_metadata(None).await {
                    Ok((_source, metas)) => metas.first().map(|m| m.0.clone()),
                    Err(e) => {
                        println!("metadata: ERROR {e}");
                        continue;
                    },
                }
            },
        };
        let Some(meta) = meta else {
            println!("metadata: None -- track end would log NoMetadata and ask nothing");
            continue;
        };
        println!(
            "metadata: title={:?} artist={:?} track={:?} channel={:?} url={:?}",
            meta.title, meta.artist, meta.track, meta.channel, meta.source_url
        );

        let Some(mut raw) = raw_track(&meta) else {
            println!("raw_track: None -- the ended track has no title, so nothing is asked");
            continue;
        };
        // Without a video id YouTube's Mix has nothing, so this shows what the
        // fallbacks do on their own.
        if std::env::var_os("AUTOPLAY_PROBE_NO_VIDEO_ID").is_some() {
            raw.video_id = None;
        }
        println!("raw_track: {raw:?}");
        println!(
            "title-parse seed: {:?}",
            TitleParseResolver::new().resolve(&raw).await
        );
        match MusicBrainz::new("cycle.five@proton.me") {
            Ok(mb) => println!("musicbrainz seed: {:?}", mb.resolve(&raw).await),
            Err(e) => println!("musicbrainz: could not build: {e}"),
        }
        // MusicBrainz allows one request a second per IP, and `reco` holds its
        // own MusicBrainz instance that knows nothing of the call above.
        tokio::time::sleep(Duration::from_millis(1_100)).await;

        let recs = reco
            .next_tracks(&raw, REFILL_SIZE)
            .await
            .expect("next_tracks does not fail on provider errors");
        println!("next_tracks: {} recommendation(s)", recs.len());
        for r in recs.iter().take(5) {
            println!(
                "  {} - {} [{}] -> {:?}",
                r.artist,
                r.title,
                r.source,
                to_query(r)
            );
        }
    }
}
