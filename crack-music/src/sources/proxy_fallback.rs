//! Serve a login- or age-gated YouTube video through the residential proxy.
//!
//! [`ProxyFallback`] wraps a lazy source. When the inner source fails with a
//! gate error ([`is_gate_error`]) it builds the same video through the proxy
//! -- once -- and serves this and every later call from that source.
//!
//! One proxied source means one [`ProxySession`](crate::proxy::ProxySession)
//! (`session()` mints a fresh id per call), so the resolve and the stream leave
//! from the same residential IP; a different IP per leg would get the
//! googlevideo URL refused. Never log the wrapped sources: `YoutubeDl`'s
//! `Debug` prints the authenticated proxy URL, so this type has no `Debug`.
use async_trait::async_trait;
use songbird::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, Input};
use symphonia::core::io::MediaSource;

use crate::proxy::{gate::is_gate_error, provider};

/// Builds the proxied source. Called at most once per [`ProxyFallback`].
pub(crate) type ProxiedFactory =
    Box<dyn FnMut() -> Result<Box<dyn Compose>, AudioStreamError> + Send>;

type Stream = Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>;

/// How many fresh proxy sessions a gated source may try. Only some exit IPs
/// clear YouTube's bot check, so one session is not enough.
const MAX_PROXY_ATTEMPTS: usize = 5;

/// After a direct failure `$err`: if it is a gate error, try up to
/// [`MAX_PROXY_ATTEMPTS`] fresh proxied sources, running `$op` (an expression
/// using `$p`) on each. The first success is stored in `$self.proxied`, locking
/// its one session for every later call; gate failures discard the source and
/// try again; any other failure is returned at once.
macro_rules! fail_over {
    ($self:ident, $err:ident, $p:ident => $op:expr) => {{
        if !is_gate_error(&$err.to_string()) {
            return Err($err);
        }
        tracing::info!("source gated, retrying through the residential proxy");
        let mut last = $err;
        for _ in 0..MAX_PROXY_ATTEMPTS {
            let mut $p = ($self.make_proxied)()?;
            match $op {
                Ok(v) => {
                    $self.proxied = Some($p);
                    return Ok(v);
                },
                Err(e) if is_gate_error(&e.to_string()) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }};
}

/// A source that falls over to the residential proxy on a gate error.
pub struct ProxyFallback {
    inner: Box<dyn Compose>,
    make_proxied: ProxiedFactory,
    proxied: Option<Box<dyn Compose>>,
}

fn fail(e: impl std::error::Error + Send + Sync + 'static) -> AudioStreamError {
    AudioStreamError::Fail(Box::new(e))
}

impl ProxyFallback {
    /// Test seam: a fallback with an injected proxied-source factory.
    pub(crate) fn from_parts(inner: Box<dyn Compose>, make_proxied: ProxiedFactory) -> Self {
        Self {
            inner,
            make_proxied,
            proxied: None,
        }
    }
}

#[async_trait]
impl Compose for ProxyFallback {
    fn create(&mut self) -> Stream {
        if let Some(p) = self.proxied.as_mut() {
            return p.create();
        }
        match self.inner.create() {
            Err(e) => fail_over!(self, e, p => p.create()),
            ok => ok,
        }
    }

    async fn create_async(&mut self) -> Stream {
        if let Some(p) = self.proxied.as_mut() {
            return p.create_async().await;
        }
        match self.inner.create_async().await {
            Err(e) => fail_over!(self, e, p => p.create_async().await),
            ok => ok,
        }
    }

    fn should_create_async(&self) -> bool {
        self.proxied.as_ref().map_or_else(
            || self.inner.should_create_async(),
            |p| p.should_create_async(),
        )
    }

    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        if let Some(p) = self.proxied.as_mut() {
            return p.aux_metadata().await;
        }
        match self.inner.aux_metadata().await {
            Err(e) => fail_over!(self, e, p => p.aux_metadata().await),
            ok => ok,
        }
    }
}

/// The production factory: ONE session per call, both the reqwest proxy and
/// the yt-dlp `--proxy` derived from it, so both legs share one exit IP.
fn production_factory(video_url: String) -> ProxiedFactory {
    Box::new(move || {
        let p = provider().expect("provider present");
        let s = p.session();
        let proxy = s.reqwest_proxy().map_err(fail)?;
        let client = reqwest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(fail)?;
        let yt = crack_types::http::ytdl_for_url_proxied(client, &video_url, s.ytdl_proxy_url())
            .map_err(fail)?;
        Ok(Box::new(yt) as Box<dyn Compose>)
    })
}

/// `input` with its lazy source wrapped in [`ProxyFallback`], or `input`
/// unchanged when no residential proxy is configured.
#[must_use]
pub fn with_proxy_fallback(input: Input, video_url: String) -> Input {
    if provider().is_none() {
        return input;
    }
    let wrap = |inner| {
        Box::new(ProxyFallback::from_parts(
            inner,
            production_factory(video_url),
        ))
    };
    match input {
        Input::Lazy(inner) => Input::Lazy(wrap(inner)),
        Input::Live(live, Some(inner)) => Input::Live(live, Some(wrap(inner))),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    const GATE: &str = "ERROR: [youtube] x: This video requires login";

    /// A source that always fails with `err` (or opens if `None`), counting calls.
    struct Scripted {
        err: Option<&'static str>,
        calls: Arc<AtomicUsize>,
    }

    impl Scripted {
        fn stream(&self) -> Stream {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.err {
                Some(m) => Err(AudioStreamError::Fail(m.to_owned().into())),
                None => Ok(AudioStream {
                    input: Box::new(std::io::Cursor::new(Vec::<u8>::new())),
                }),
            }
        }
    }

    #[async_trait]
    impl Compose for Scripted {
        fn create(&mut self) -> Stream {
            self.stream()
        }
        async fn create_async(&mut self) -> Stream {
            self.stream()
        }
        fn should_create_async(&self) -> bool {
            true
        }
        async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.err {
                Some(m) => Err(AudioStreamError::Fail(m.to_owned().into())),
                None => Ok(AuxMetadata::default()),
            }
        }
    }

    fn scripted(err: Option<&'static str>) -> (Box<dyn Compose>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Box::new(Scripted {
                err,
                calls: calls.clone(),
            }),
            calls,
        )
    }

    /// A factory whose n-th build fails with `errs[n]`, then opens; with
    /// `always` set, every build fails with the last of `errs`.
    fn seq_factory(errs: Vec<&'static str>, always: bool) -> (ProxiedFactory, Arc<AtomicUsize>) {
        let builds = Arc::new(AtomicUsize::new(0));
        let b = builds.clone();
        (
            Box::new(move || {
                let n = b.fetch_add(1, Ordering::SeqCst);
                let err = if always { errs.last() } else { errs.get(n) };
                Ok(scripted(err.copied()).0)
            }),
            builds,
        )
    }

    fn counting_factory() -> (ProxiedFactory, Arc<AtomicUsize>) {
        seq_factory(vec![], false)
    }

    #[tokio::test]
    async fn gate_error_triggers_one_proxied_build_reused() {
        let (inner, _) = scripted(Some(GATE));
        let (factory, builds) = counting_factory();
        let mut src = ProxyFallback::from_parts(inner, factory);
        assert!(src.aux_metadata().await.is_ok());
        assert!(src.create_async().await.is_ok());
        assert!(src.create().is_ok());
        assert_eq!(builds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_gate_error_propagates() {
        let (inner, _) = scripted(Some("HTTP Error 404: Not Found"));
        let (factory, builds) = counting_factory();
        let mut src = ProxyFallback::from_parts(inner, factory);
        let Err(err) = src.create_async().await else {
            panic!("should fail");
        };
        assert!(err.to_string().contains("404"));
        assert!(src.aux_metadata().await.is_err());
        assert_eq!(builds.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn no_provider_returns_input_unchanged() {
        assert!(provider().is_none(), "tests run without proxy env");
        let (inner, calls) = scripted(Some(GATE));
        let Input::Lazy(mut src) = with_proxy_fallback(Input::Lazy(inner), "u".into()) else {
            panic!("still lazy");
        };
        // Unwrapped: the gate error reaches us from the raw source, once.
        assert!(src.create_async().await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn gate_retries_until_a_clean_session() {
        let (inner, _) = scripted(Some(GATE));
        let (factory, builds) = seq_factory(vec![GATE, GATE], false);
        let mut src = ProxyFallback::from_parts(inner, factory);
        assert!(src.aux_metadata().await.is_ok());
        assert_eq!(builds.load(Ordering::SeqCst), 3);
        assert!(src.create_async().await.is_ok());
        assert!(src.create().is_ok());
        assert_eq!(builds.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts() {
        let (inner, _) = scripted(Some(GATE));
        let (factory, builds) = seq_factory(vec![GATE], true);
        let mut src = ProxyFallback::from_parts(inner, factory);
        let Err(err) = src.create_async().await else {
            panic!("should fail");
        };
        assert!(is_gate_error(&err.to_string()));
        assert_eq!(builds.load(Ordering::SeqCst), MAX_PROXY_ATTEMPTS);
    }

    #[tokio::test]
    async fn non_gate_error_from_proxy_attempt_propagates() {
        let (inner, _) = scripted(Some(GATE));
        let (factory, builds) = seq_factory(vec![GATE, "HTTP Error 404: Not Found"], false);
        let mut src = ProxyFallback::from_parts(inner, factory);
        let Err(err) = src.create_async().await else {
            panic!("should fail");
        };
        assert!(err.to_string().contains("404"));
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }
}
