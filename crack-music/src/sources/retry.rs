//! One more try for a stream whose URL was refused.
//!
//! YouTube intermittently answers the googlevideo URL yt-dlp just resolved
//! with `403 Forbidden`: 3 of ~45 track starts on production on 2026-10-05/06,
//! while the same resolve-then-fetch, repeated by hand straight afterwards,
//! served four times out of four. songbird drops such a track before a frame
//! plays and the queue moves on.
//!
//! [`RetryRefused`] wraps a lazy source and, when opening it fails on an HTTP
//! status, opens it once more. For yt-dlp that is a fresh resolve -- songbird's
//! `YoutubeDl::create_async` runs yt-dlp on every call -- so the retry asks for
//! a new URL rather than repeating the refused one. A second refusal is final
//! and reaches the channel as `messaging::track_failed`'s notice.
use async_trait::async_trait;
use songbird::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, Input};
use symphonia::core::io::MediaSource;

/// The start of the error songbird's `HttpRequest` returns for a non-2xx
/// response (`input/sources/http.rs` at the pinned revision). It is a string
/// boxed as an error, so the text is all there is to match on; a test below
/// pins it against songbird itself.
const REFUSED: &str = "failed with http status code:";

/// Did the source fail because the server refused the stream URL?
fn refused(err: &AudioStreamError) -> bool {
    matches!(err, AudioStreamError::Fail(why) if why.to_string().starts_with(REFUSED))
}

/// A source that is opened a second time if its URL is refused.
pub struct RetryRefused {
    inner: Box<dyn Compose>,
}

#[async_trait]
impl Compose for RetryRefused {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        match self.inner.create() {
            Err(err) if refused(&err) => {
                tracing::info!("stream refused ({err}), resolving once more");
                self.inner.create()
            },
            other => other,
        }
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        match self.inner.create_async().await {
            Err(err) if refused(&err) => {
                tracing::info!("stream refused ({err}), resolving once more");
                self.inner.create_async().await
            },
            other => other,
        }
    }

    fn should_create_async(&self) -> bool {
        self.inner.should_create_async()
    }

    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        self.inner.aux_metadata().await
    }
}

/// `input`, with its lazy source -- if it has one -- wrapped in [`RetryRefused`].
/// A live input keeps its composer wrapped too: songbird re-creates from it to
/// seek, which fetches the URL again.
#[must_use]
pub fn retry_refused(input: Input) -> Input {
    match input {
        Input::Lazy(inner) => Input::Lazy(Box::new(RetryRefused { inner })),
        Input::Live(live, Some(inner)) => Input::Live(live, Some(Box::new(RetryRefused { inner }))),
        other => other,
    }
}

/// A source that refuses once and then opens, and a count of its opens.
#[cfg(test)]
pub(crate) fn refuses_once() -> (
    Box<dyn Compose>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let inner = Box::new(tests::Scripted {
        errors: vec![tests::fail("failed with http status code: 403 Forbidden")],
        calls: calls.clone(),
    });
    (inner, calls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    pub(super) fn fail(msg: &str) -> AudioStreamError {
        AudioStreamError::Fail(msg.to_owned().into())
    }

    /// A source that fails with each of `errors` in turn, then opens.
    pub(super) struct Scripted {
        pub(super) errors: Vec<AudioStreamError>,
        pub(super) calls: Arc<AtomicUsize>,
    }

    impl Scripted {
        fn next(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.errors.is_empty() {
                Ok(AudioStream {
                    input: Box::new(std::io::Cursor::new(Vec::<u8>::new())),
                })
            } else {
                Err(self.errors.remove(0))
            }
        }
    }

    #[async_trait]
    impl Compose for Scripted {
        fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
            self.next()
        }
        async fn create_async(
            &mut self,
        ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
            self.next()
        }
        fn should_create_async(&self) -> bool {
            true
        }
    }

    fn wrapped(errors: Vec<AudioStreamError>) -> (RetryRefused, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let inner = Box::new(Scripted {
            errors,
            calls: calls.clone(),
        });
        (RetryRefused { inner }, calls)
    }

    #[tokio::test]
    async fn a_refused_stream_is_opened_once_more() {
        let (mut source, calls) =
            wrapped(vec![fail("failed with http status code: 403 Forbidden")]);
        assert!(source.create_async().await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_second_refusal_is_final() {
        let refusal = || fail("failed with http status code: 403 Forbidden");
        let (mut source, calls) = wrapped(vec![refusal(), refusal(), refusal()]);
        let Err(err) = source.create_async().await else {
            panic!("a second refusal should fail");
        };
        assert!(refused(&err), "{err}");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A video yt-dlp cannot resolve stays dead: asking again would only
    /// delay the next track by another yt-dlp run.
    #[tokio::test]
    async fn other_failures_are_not_retried() {
        let (mut source, calls) = wrapped(vec![fail("ERROR: Video unavailable")]);
        assert!(source.create_async().await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_blocking_path_retries_too() {
        let (mut source, calls) = wrapped(vec![fail(
            "failed with http status code: 503 Service Unavailable",
        )]);
        assert!(source.create().is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    fn empty_live() -> songbird::input::LiveInput {
        songbird::input::LiveInput::Raw(AudioStream {
            input: Box::new(std::io::Cursor::new(Vec::<u8>::new())),
        })
    }

    #[tokio::test]
    async fn a_lazy_source_is_wrapped() {
        let (inner, calls) = refuses_once();
        let Input::Lazy(mut source) = retry_refused(Input::Lazy(inner)) else {
            panic!("still lazy");
        };
        assert!(source.create_async().await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// songbird re-creates a live input from its composer to seek.
    #[tokio::test]
    async fn a_live_inputs_composer_is_wrapped() {
        let (inner, calls) = refuses_once();
        let Input::Live(_, Some(mut source)) =
            retry_refused(Input::Live(empty_live(), Some(inner)))
        else {
            panic!("still live, still with a composer");
        };
        assert!(source.create_async().await.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(matches!(
            retry_refused(Input::Live(empty_live(), None)),
            Input::Live(_, None)
        ));
    }

    /// Pins [`REFUSED`] against songbird itself: a real `HttpRequest` against
    /// a local server that answers 403. If a songbird bump rewords the error,
    /// this fails rather than the retry going quietly dead.
    #[tokio::test]
    async fn songbirds_own_refusal_is_recognised() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
        });
        let mut http = songbird::input::HttpRequest::new(
            crate::http_utils::get_client_old().clone(),
            format!("http://{addr}/s"),
        );
        let Err(err) = http.create_async().await else {
            panic!("a 403 should fail");
        };
        assert!(refused(&err), "{err}");
    }
}
