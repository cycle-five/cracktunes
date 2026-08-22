//! Compares playlist resolution paths end to end.
//!
//! Run with: cargo run -p crack-testing --example playlist_bench -- <playlist-url>
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://www.youtube.com/playlist?list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP".into());
    let limit: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(100);

    let client = crack_testing::build_configured_reqwest_client();

    println!("== rusty_ytdl (what the bot used before) ==");
    let t = Instant::now();
    match rusty_ytdl::search::Playlist::get(&url, None).await {
        Ok(p) => println!("  {} videos in {:?}", p.videos.len(), t.elapsed()),
        Err(e) => println!("  FAILED after {:?}: {e}", t.elapsed()),
    }

    println!("== yt_playlist page reader (new) ==");
    let t = Instant::now();
    let entries = crack_testing::fetch_playlist(&client, &url, limit).await?;
    let page_time = t.elapsed();
    println!("  {} entries in {page_time:?}", entries.len());
    for e in entries.iter().take(3) {
        println!("    - {} [{:?}] {}", e.title, e.duration, e.url());
    }

    println!("== full resolve_playlist_limit (metadata ready to queue) ==");
    let ct = crack_testing::CrackTrackClient::new_with_req_client(client.clone());
    let t = Instant::now();
    let tracks = ct.resolve_playlist_limit(&url, limit as u64).await?;
    println!("  {} tracks in {:?}", tracks.len(), t.elapsed());

    // What the old play path did: one metadata round trip per track, serially.
    let sample = tracks.len().min(8);
    println!("== old per-track resolution, serial (sampling {sample} tracks) ==");
    let t = Instant::now();
    let mut ok = 0;
    for track in tracks.iter().take(sample) {
        if ct
            .resolve_track(crack_types::QueryType::VideoLink(track.get_url()))
            .await
            .is_ok()
        {
            ok += 1;
        }
    }
    let serial = t.elapsed();
    println!("  {ok}/{sample} resolved in {serial:?}");
    if sample > 0 {
        let per = serial / sample as u32;
        println!(
            "  ~{per:?}/track => a {}-track playlist would take ~{:?} that way",
            tracks.len(),
            per * tracks.len() as u32
        );
        println!("  new path took {page_time:?} for all {} tracks", tracks.len());
    }
    Ok(())
}
