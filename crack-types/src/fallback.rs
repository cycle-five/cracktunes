//! Searching YouTube: rusty_ytdl first, yt-dlp whenever it has no answer.

use std::fmt::Display;
use std::future::Future;

/// rusty_ytdl's hit for `query` if it found one, otherwise whatever `ytdlp`
/// finds, with the reason logged.
///
/// 🪤 rusty_ytdl talks to YouTube's private API and breaks when YouTube changes
/// it, as its player requests did on 2026-09-05 (400s). Its search used to fall
/// back to yt-dlp only when it found *nothing*: an error returned early, and the
/// fallback after that (`ready_query`) searched with rusty_ytdl again. yt-dlp is
/// slower but kept up to date, so it answers whenever rusty_ytdl can't: an error
/// and an empty result alike. Here the rusty_ytdl error can only be logged: the
/// signature has no way to turn `E` into `X`.
pub async fn or_ask_ytdlp<T, E, X, F, Fut>(
    query: &str,
    rusty: Result<Option<T>, E>,
    ytdlp: F,
) -> Result<T, X>
where
    E: Display,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, X>>,
{
    match rusty {
        Ok(Some(hit)) => Ok(hit),
        Ok(None) => {
            tracing::warn!("rusty_ytdl found nothing for {query:?}; asking yt-dlp");
            ytdlp().await
        },
        Err(e) => {
            tracing::warn!("rusty_ytdl could not search for {query:?}; asking yt-dlp: {e}");
            ytdlp().await
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::cell::Cell;

    /// Runs `or_ask_ytdlp` with a yt-dlp stand-in that answers `"ytdlp"` and
    /// counts its calls.
    async fn run(
        rusty: Result<Option<&'static str>, &'static str>,
    ) -> (Result<&'static str, ()>, u32) {
        let calls = Cell::new(0);
        let got = or_ask_ytdlp("q", rusty, || async {
            calls.set(calls.get() + 1);
            Ok("ytdlp")
        })
        .await;
        (got, calls.get())
    }

    #[tokio::test]
    async fn a_rusty_hit_is_used_and_ytdlp_never_runs() {
        assert_eq!(run(Ok(Some("rusty"))).await, (Ok("rusty"), 0));
    }

    #[tokio::test]
    async fn an_empty_rusty_result_asks_ytdlp() {
        assert_eq!(run(Ok(None)).await, (Ok("ytdlp"), 1));
    }

    #[tokio::test]
    async fn a_rusty_error_asks_ytdlp() {
        assert_eq!(run(Err("youtubei 400")).await, (Ok("ytdlp"), 1));
    }

    #[tokio::test]
    async fn ytdlps_own_error_comes_back() {
        let got: Result<&str, &str> =
            or_ask_ytdlp("q", Err::<Option<&str>, _>("rusty down"), || async {
                Err("ytdlp down")
            })
            .await;
        assert_eq!(got, Err("ytdlp down"));
    }
}
