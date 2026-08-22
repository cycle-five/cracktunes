//! Direct YouTube playlist page reader.
//!
//! `rusty_ytdl` 0.7.4 locates playlist entries by looking for
//! `playlistVideoRenderer` objects inside `ytInitialData`. YouTube has since
//! migrated playlist listings to its "view model" shape, where each entry is a
//! `lockupViewModel`, so that lookup now finds nothing and playlist resolution
//! fails outright with `PlaylistBodyCannotParsed` -- i.e. playlists stopped
//! resolving at all, not merely slowly.
//!
//! This module reads the playlist page directly and understands both shapes, so
//! it keeps working whichever one YouTube serves. Everything here is metadata
//! only: no per-track network calls, no stream resolution. That happens lazily
//! at playback time.

use crack_types::Error;
use serde_json::Value;
use std::time::Duration;

/// Browser-ish UA. YouTube serves the lightweight/no-JS shell to unknown
/// clients, which does not embed `ytInitialData` at all.
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Fallback innertube key, used only if the page does not embed one.
const FALLBACK_API_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";
const FALLBACK_CLIENT_VERSION: &str = "2.20240814.00.00";

/// Hard ceiling on continuation requests, so a pathological playlist cannot
/// spin forever.
const MAX_CONTINUATIONS: usize = 20;

/// One entry of a YouTube playlist, as listed on the playlist page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub video_id: String,
    pub title: String,
    pub duration: Option<Duration>,
    pub thumbnail: Option<String>,
    pub channel: Option<String>,
}

impl PlaylistEntry {
    /// The canonical watch URL for this entry.
    #[must_use]
    pub fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.video_id)
    }
}

/// Fetch up to `limit` entries of the playlist at `url`.
///
/// Makes one request for the first page and then one per continuation, so a
/// 200-track playlist costs 2-3 requests total rather than one per track.
pub async fn fetch_playlist(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<PlaylistEntry>, Error> {
    let body = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        // Ask for English so duration/label parsing stays predictable.
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let data = extract_yt_initial_data(&body)
        .ok_or_else(|| -> Error { "playlist page did not contain ytInitialData".into() })?;

    let mut entries = Vec::new();
    collect_entries(&data, &mut entries);

    let api_key = scrape_between(&body, "\"INNERTUBE_API_KEY\":\"", '"')
        .unwrap_or_else(|| FALLBACK_API_KEY.to_string());
    let client_version = scrape_between(&body, "\"INNERTUBE_CLIENT_VERSION\":\"", '"')
        .unwrap_or_else(|| FALLBACK_CLIENT_VERSION.to_string());

    let mut token = find_continuation_token(&data);
    let mut rounds = 0;
    while entries.len() < limit && rounds < MAX_CONTINUATIONS {
        let Some(t) = token.take() else { break };
        rounds += 1;
        let next = match fetch_continuation(client, &api_key, &client_version, &t).await {
            Ok(next) => next,
            Err(e) => {
                // Partial results beat no results.
                tracing::warn!("playlist continuation failed after {} entries: {e}", entries.len());
                break;
            },
        };
        let before = entries.len();
        collect_entries(&next, &mut entries);
        if entries.len() == before {
            break;
        }
        token = find_continuation_token(&next);
    }

    dedup_by_video_id(&mut entries);
    entries.truncate(limit);
    Ok(entries)
}

/// POST one continuation to the innertube `browse` endpoint.
async fn fetch_continuation(
    client: &reqwest::Client,
    api_key: &str,
    client_version: &str,
    token: &str,
) -> Result<Value, Error> {
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": client_version,
                "hl": "en",
                "gl": "US",
            }
        },
        "continuation": token,
    });

    let res = client
        .post(format!(
            "https://www.youtube.com/youtubei/v1/browse?key={api_key}&prettyPrint=false"
        ))
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header("X-Youtube-Client-Name", "1")
        .header("X-Youtube-Client-Version", client_version)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    Ok(res)
}

/// Pull the `ytInitialData` JSON object out of a playlist page.
///
/// YouTube writes it as either `var ytInitialData = {...};` or
/// `window["ytInitialData"] = {...};` depending on the variant served, so we
/// find the assignment and then scan for the matching closing brace rather than
/// relying on a greedy regex.
fn extract_yt_initial_data(html: &str) -> Option<Value> {
    for marker in ["var ytInitialData = ", "window[\"ytInitialData\"] = "] {
        if let Some(start) = html.find(marker) {
            let rest = &html[start + marker.len()..];
            if let Some(obj) = take_json_object(rest) {
                if let Ok(v) = serde_json::from_str::<Value>(obj) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Return the leading `{...}` of `s`, respecting nesting, strings and escapes.
fn take_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[..=i]);
                }
            },
            _ => {},
        }
    }
    None
}

/// Grab the text between `prefix` and the next `terminator`.
fn scrape_between(haystack: &str, prefix: &str, terminator: char) -> Option<String> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find(terminator)?;
    Some(rest[..end].to_string())
}

/// Walk the response and pick up every playlist entry, in document order.
fn collect_entries(value: &Value, out: &mut Vec<PlaylistEntry>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    // Current shape.
                    "lockupViewModel" => {
                        if let Some(entry) = parse_lockup(child) {
                            out.push(entry);
                            continue;
                        }
                    },
                    // Pre-2024 shape, kept so we still work if YouTube serves it.
                    "playlistVideoRenderer" => {
                        if let Some(entry) = parse_legacy(child) {
                            out.push(entry);
                            continue;
                        }
                    },
                    _ => {},
                }
                collect_entries(child, out);
            }
        },
        Value::Array(items) => {
            for item in items {
                collect_entries(item, out);
            }
        },
        _ => {},
    }
}

/// Parse a `lockupViewModel` entry.
fn parse_lockup(v: &Value) -> Option<PlaylistEntry> {
    // Playlists can embed non-video lockups (channels, nested playlists).
    match v.get("contentType").and_then(Value::as_str) {
        Some("LOCKUP_CONTENT_TYPE_VIDEO") | None => {},
        Some(_) => return None,
    }
    let video_id = v.get("contentId")?.as_str()?.to_string();
    let title = v
        .pointer("/metadata/lockupMetadataViewModel/title/content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // The duration rides along as a thumbnail overlay badge, e.g. "3:00".
    let duration = find_first(v, "thumbnailBadgeViewModel")
        .and_then(|b| b.get("text").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        .and_then(parse_duration);

    let thumbnail = v
        .pointer("/contentImage/thumbnailViewModel/image/sources")
        .and_then(Value::as_array)
        .and_then(|s| largest_thumbnail(s));

    let channel = find_first(v, "decoratedAvatarViewModel")
        .and_then(|a| a.get("a11yLabel").and_then(Value::as_str))
        .map(|s| s.trim_start_matches("Go to channel ").to_string());

    Some(PlaylistEntry {
        video_id,
        title,
        duration,
        thumbnail,
        channel,
    })
}

/// Parse a legacy `playlistVideoRenderer` entry.
fn parse_legacy(v: &Value) -> Option<PlaylistEntry> {
    let video_id = v.get("videoId")?.as_str()?.to_string();
    let title = v
        .pointer("/title/runs/0/text")
        .or_else(|| v.pointer("/title/simpleText"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let duration = v
        .pointer("/lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .or_else(|| {
            v.pointer("/lengthText/simpleText")
                .and_then(Value::as_str)
                .and_then(parse_duration)
        });
    let thumbnail = v
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|s| largest_thumbnail(s));
    let channel = v
        .pointer("/shortBylineText/runs/0/text")
        .and_then(Value::as_str)
        .map(str::to_owned);

    Some(PlaylistEntry {
        video_id,
        title,
        duration,
        thumbnail,
        channel,
    })
}

/// Pick the widest thumbnail source.
fn largest_thumbnail(sources: &[Value]) -> Option<String> {
    sources
        .iter()
        .max_by_key(|s| s.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|s| s.get("url").and_then(Value::as_str))
        .map(str::to_owned)
}

/// Depth-first search for the first object stored under `key`.
fn find_first<'v>(value: &'v Value, key: &str) -> Option<&'v Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|v| find_first(v, key))
        },
        Value::Array(items) => items.iter().find_map(|v| find_first(v, key)),
        _ => None,
    }
}

/// Locate the next continuation token, if the listing has one.
///
/// YouTube moved these to a view model too: the token used to sit at
/// `continuationItemRenderer.continuationEndpoint.continuationCommand.token`
/// and now sits under `continuationItemViewModel`, one `innertubeCommand`
/// deeper. Both are handled by finding the continuation item and then the
/// first `token` string inside it.
fn find_continuation_token(value: &Value) -> Option<String> {
    for key in ["continuationItemViewModel", "continuationItemRenderer"] {
        if let Some(item) = find_first(value, key) {
            if let Some(token) = find_first_string(item, "token") {
                return Some(token);
            }
        }
    }
    None
}

/// Depth-first search for the first string stored under `key`.
fn find_first_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(s) = map.get(key).and_then(Value::as_str) {
                return Some(s.to_owned());
            }
            map.values().find_map(|v| find_first_string(v, key))
        },
        Value::Array(items) => items.iter().find_map(|v| find_first_string(v, key)),
        _ => None,
    }
}

/// Parse `"m:ss"`, `"h:mm:ss"`, or `"ss"`. Non-numeric labels such as "LIVE"
/// yield `None`, which callers treat as "unknown duration".
fn parse_duration(text: &str) -> Option<Duration> {
    let mut secs: u64 = 0;
    let mut any = false;
    for part in text.trim().split(':') {
        let n: u64 = part.trim().parse().ok()?;
        secs = secs.checked_mul(60)?.checked_add(n)?;
        any = true;
    }
    any.then(|| Duration::from_secs(secs))
}

/// Drop repeated entries, keeping first-seen order.
///
/// Continuations can overlap at the boundary, and a playlist may legitimately
/// list the same video twice; neither should produce duplicate queue entries.
fn dedup_by_video_id(entries: &mut Vec<PlaylistEntry>) {
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.video_id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("3:00"), Some(Duration::from_secs(180)));
        assert_eq!(parse_duration("1:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(parse_duration("45"), Some(Duration::from_secs(45)));
        assert_eq!(parse_duration("LIVE"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn test_take_json_object() {
        assert_eq!(take_json_object(r#"{"a":1};rest"#), Some(r#"{"a":1}"#));
        // Braces inside strings must not confuse the scanner.
        assert_eq!(take_json_object(r#"{"a":"}"};x"#), Some(r#"{"a":"}"}"#));
        assert_eq!(take_json_object(r#"{"a":"\""};x"#), Some(r#"{"a":"\""}"#));
        assert_eq!(take_json_object(r#"{"a":{"b":2}} "#), Some(r#"{"a":{"b":2}}"#));
        assert_eq!(take_json_object("[1,2]"), None);
        assert_eq!(take_json_object(r#"{"a":1"#), None);
    }

    #[test]
    fn test_parse_lockup() {
        let v: Value = serde_json::from_str(
            r#"{
                "contentId": "abc123",
                "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
                "metadata": {"lockupMetadataViewModel": {"title": {"content": "A Song"}}},
                "contentImage": {"thumbnailViewModel": {
                    "image": {"sources": [
                        {"url": "small.jpg", "width": 168},
                        {"url": "big.jpg", "width": 336}
                    ]},
                    "overlays": [{"thumbnailBottomOverlayViewModel": {"badges": [
                        {"thumbnailBadgeViewModel": {"text": "3:00"}}
                    ]}}]
                }}
            }"#,
        )
        .unwrap();
        let entry = parse_lockup(&v).expect("should parse");
        assert_eq!(entry.video_id, "abc123");
        assert_eq!(entry.title, "A Song");
        assert_eq!(entry.duration, Some(Duration::from_secs(180)));
        assert_eq!(entry.thumbnail.as_deref(), Some("big.jpg"));
        assert_eq!(entry.url(), "https://www.youtube.com/watch?v=abc123");
    }

    #[test]
    fn test_parse_lockup_skips_non_video() {
        let v: Value = serde_json::from_str(
            r#"{"contentId": "x", "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST"}"#,
        )
        .unwrap();
        assert!(parse_lockup(&v).is_none());
    }

    #[test]
    fn test_parse_legacy() {
        let v: Value = serde_json::from_str(
            r#"{
                "videoId": "old123",
                "title": {"runs": [{"text": "Legacy Song"}]},
                "lengthSeconds": "241",
                "thumbnail": {"thumbnails": [{"url": "t.jpg", "width": 120}]},
                "shortBylineText": {"runs": [{"text": "Some Channel"}]}
            }"#,
        )
        .unwrap();
        let entry = parse_legacy(&v).expect("should parse");
        assert_eq!(entry.video_id, "old123");
        assert_eq!(entry.title, "Legacy Song");
        assert_eq!(entry.duration, Some(Duration::from_secs(241)));
        assert_eq!(entry.channel.as_deref(), Some("Some Channel"));
    }

    #[test]
    fn test_collect_entries_handles_both_shapes() {
        let v: Value = serde_json::from_str(
            r#"{"contents": [
                {"lockupViewModel": {"contentId": "new1", "contentType": "LOCKUP_CONTENT_TYPE_VIDEO",
                 "metadata": {"lockupMetadataViewModel": {"title": {"content": "New"}}}}},
                {"playlistVideoRenderer": {"videoId": "old1", "title": {"simpleText": "Old"}}}
            ]}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        collect_entries(&v, &mut out);
        assert_eq!(
            out.iter().map(|e| e.video_id.as_str()).collect::<Vec<_>>(),
            vec!["new1", "old1"]
        );
    }

    #[test]
    fn test_find_continuation_token_both_shapes() {
        let new_shape: Value = serde_json::from_str(
            r#"{"contents":[{"continuationItemViewModel":{"continuationCommand":
               {"innertubeCommand":{"continuationCommand":{"token":"TOKEN_NEW"}}}}}]}"#,
        )
        .unwrap();
        assert_eq!(
            find_continuation_token(&new_shape),
            Some("TOKEN_NEW".to_string())
        );

        let legacy: Value = serde_json::from_str(
            r#"{"contents":[{"continuationItemRenderer":{"continuationEndpoint":
               {"continuationCommand":{"token":"TOKEN_OLD"}}}}]}"#,
        )
        .unwrap();
        assert_eq!(
            find_continuation_token(&legacy),
            Some("TOKEN_OLD".to_string())
        );

        let none: Value = serde_json::from_str(r#"{"contents":[]}"#).unwrap();
        assert_eq!(find_continuation_token(&none), None);
    }

    #[test]
    fn test_dedup_preserves_order() {
        let mk = |id: &str| PlaylistEntry {
            video_id: id.to_string(),
            title: String::new(),
            duration: None,
            thumbnail: None,
            channel: None,
        };
        let mut v = vec![mk("a"), mk("b"), mk("a"), mk("c")];
        dedup_by_video_id(&mut v);
        assert_eq!(
            v.iter().map(|e| e.video_id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }
}
