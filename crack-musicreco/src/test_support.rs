//! Shared test-only mock HTTP server. Every provider's tests use this instead
//! of rolling their own -- a fix to the harness (a missed header, a body not
//! read to completion) then fixes every provider's tests at once instead of
//! only the one that happened to get patched.
#![cfg(test)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One canned response: status, body, and any extra headers.
///
/// 🪤 A header value may contain the literal placeholder `"{addr}"`,
/// substituted with the mock server's own `host:port` at serve time -- e.g. a
/// `Location` that must point back at the same server, which isn't known
/// until after the listener binds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Canned {
    pub status: u16,
    pub body: &'static str,
    pub headers: &'static [(&'static str, &'static str)],
}

/// The common case: a plain `(status, body)` pair, no extra headers.
impl From<(u16, &'static str)> for Canned {
    fn from((status, body): (u16, &'static str)) -> Self {
        Self {
            status,
            body,
            headers: &[],
        }
    }
}

/// Serves canned responses in order over a local TCP listener. Requests past
/// the end of the list get a bare `200 {}`.
///
/// Returns the base url, a request counter, and the raw text of every request
/// received.
///
/// Counting REQUESTS is the point: a provider that silently skips or
/// double-calls still returns a plausible value. Keeping the request TEXT is
/// the same idea one level down -- the only way to assert on what we actually
/// sent, rather than on what we got back.
pub(crate) async fn serve<T: Into<Canned>>(
    responses: Vec<T>,
) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let responses: Vec<Canned> = responses.into_iter().map(Into::into).collect();
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = l.local_addr().expect("local_addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let served = Arc::clone(&hits);
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let n = served.fetch_add(1, Ordering::SeqCst);
            let canned = responses.get(n).copied().unwrap_or(Canned {
                status: 200,
                body: "{}",
                headers: &[],
            });

            // 🪤 Read until the end of the request head rather than once. A
            // single `read` is not guaranteed to deliver it all -- that holds
            // today only because these payloads are tiny and travel over
            // loopback, which is a property of the test environment, not of
            // TCP.
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                match s.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        raw.extend_from_slice(&buf[..n]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    },
                    Err(_) => break,
                }
            }
            recorded
                .lock()
                .expect("test mutex")
                .push(String::from_utf8_lossy(&raw).into_owned());

            let mut head = format!("HTTP/1.1 {} X\r\n", canned.status);
            for (name, value) in canned.headers {
                head.push_str(&format!(
                    "{name}: {}\r\n",
                    value.replace("{addr}", &addr.to_string())
                ));
            }
            head.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                canned.body.len(),
                canned.body
            ));
            let _ = s.write_all(head.as_bytes()).await;
            let _ = s.shutdown().await;
        }
    });
    (format!("http://{addr}"), hits, seen)
}

/// A listener that never accepts, reads, or answers -- for exercising a
/// client's own request timeout in isolation from an actual slow upstream.
///
/// 🔑 No `accept()` loop is needed: the OS completes the TCP handshake into
/// the listen backlog on its own, so a client's `connect()` succeeds and the
/// connection then simply never produces a response. The listener is leaked
/// (not dropped) so the port stays open for the test's lifetime without
/// anything servicing it.
pub(crate) async fn accept_and_hang() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = l.local_addr().expect("local_addr");
    std::mem::forget(l);
    format!("http://{addr}")
}
