# Messaging Layer (PR 1, v0.22.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every music-facing message the bot sends goes through one typed renderer (`messaging::render`) and one delivery layer (`messaging::courier`), which clippy enforces. The known message quirks are fixed once, in that one place.

**Architecture:**
- `CrackedMessage` stays the vocabulary. Track-bearing variants carry a `TrackLabel`, and new card variants cover now-playing, finished, failed tracks, echoes and queued tracks.
- `render(&CrackedMessage, &RenderCx) -> Rendered` is pure. It is the only code that formats titles, links, durations, thumbnails, mentions and length limits.
- Delivery goes through a `Transport` trait (today's `StatusTransport`, widened) for channel messages, and a `ReplySink` trait for command replies. Both have recording fakes.
- `clippy.toml` `disallowed-methods` bans raw serenity and poise sends outside `messaging`.

**Tech Stack:** Rust 2021; serenity `next` (rev 37b9f43); poise `serenity-next` (rev b189f7c); songbird (rev 3fe7289); tokio; `url`; `async-trait`.

**Spec:** `docs/superpowers/specs/2026-10-07-messaging-layer-and-now-playing-buttons-design.md`. Read its "Architecture" and "PR 1" sections before starting any task.

## Global Constraints

- **Formatting:** the crates are edition 2021. Format with `cargo +nightly fmt --all`, never bare `rustfmt --edition 2024`, which reorders imports differently and fails CI's nightly fmt leg.
- **Lint:** `cargo clippy --workspace --all-targets` must be clean. Use `#[expect(lint, reason = "…")]`, never `#[allow]`, for new suppressions.
- **Commits:** every commit message ends with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`, and has no other Co-Authored-By line.
- **Staging:** never `git add -A` or `git add .`. Stage named paths only.
- **Serialization:** typed serde only. Never `serde_json::json!` or `serde_json::Value` for data we own. Tests may read a serialized `CreateEmbed` as a `Value`, which is a third-party shape.
- **Strings:** user-facing strings are constants in `crack-core/src/messaging/messages.rs`. Log text stays inline.
- **Third-party text:** titles, URLs, user names and error text are escaped before they reach Discord. A raw error never reaches Discord; this is the v0.17.2 leak lesson.
- **Tests:**
  - every new test is shown to fail once by breaking the code it covers, then restored (the standing rule);
  - wording is pinned against **literal strings**, never against the constant itself;
  - tests assert on what is **sent**, through the fakes.
- **`get_info`:** `TrackHandle::get_info()` never answers on an offline `Call::standalone`. Every read of it is bounded by `crate::music::ops::TRACK_INFO_TIMEOUT` (1 s, `pub(crate)`), and tests that touch it carry an outer `tokio::time::timeout`.
- **Wording:** unchanged except for the fixes this plan names.

## Rulings (plan-level; the spec is the authority)

1. **Notice is a slot, not an address.** `track_failed` keeps its coalescing slot and renders through `render` and `Transport`. There is no `Destination::Notice`, because merging happens before rendering. If wrong, this costs one enum variant added later.
2. **Replies use a generic `ReplySink`, not `Destination::Reply`.** poise's `ReplyHandle<'ctx>` is a generic handle type that an enum variant cannot carry cleanly. The spec's `Destination::Reply` becomes `courier::reply`/`courier::edit_reply` over `ReplySink`. The spec's intent is kept: one path, faked in tests.
3. **`Display` stays the per-variant text table.** It lives in `messaging/message.rs` and is inside the module. `render` is the only sender-facing entry point. Nothing outside `messaging` may call `.to_string()` on a `CrackedMessage` to send it; the ban plus review enforce this.
4. **`Mentions` has two variants, `None` (default) and `Users`.** `Users` is unused in PR 1. It exists for the follow-up that moves the welcome message.

## Review Focus

These are the inputs most likely to bite a listener that the spec implies but no single test would naturally cover. Each is pinned in the task noted.

1. **A title that is empty, only whitespace, or all markdown characters** (`""`, `"   "`, `"**"`, `"[x](y)"`, `"@everyone"`). It must render as `(untitled)` or escaped text, never `**` or a ping. Task 1 and Task 2.
2. **A URL that is relative, `javascript:`, empty, or contains `)`.** It must never become a broken masked link or an embed `url`. Task 1 and Task 4.
3. **A duration that is `None`, zero, under a second, exactly 1 h, or a live stream.** It must never show `00:00`, and `h:mm:ss` starts at 3600 s. Task 1 and Task 4.
4. **A reply in a channel without `EMBED_LINKS`.** The degraded notice must still be in `content`. Task 3 and Task 7.
5. **A track whose `get_info` never answers** (offline call). The status update must still happen, within about 1 s, rendering the progress from position 0 (`Progress::Playing { position: 0, .. }`), never hanging. Task 4.

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `crack-core/src/messaging/format.rs` (new) | `TrackLabel`, `cap`, `escape`, `clip`, `http_url`, `duration_text`, `clock`, `Progress`, `progress_text`, limit constants | 1 |
| `crack-core/src/messaging/render.rs` (new) | `Rendered`, `Mentions`, `RenderCx`, `Style`, `render`, plus conversions to `CreateReply`, `CreateMessage` and `EditMessage` | 2 |
| `crack-core/src/messaging/message.rs` | `SkipTo`/`SongQueued` carry `TrackLabel`; new card variants; `style()` | 2 |
| `crack-core/src/messaging/cards.rs` (new) | `NowPlayingCard`, `QueuedCard`, `EchoLine`, `Via`; card renderers | 2, 4, 5, 7 |
| `crack-core/src/messaging/transport.rs` (new) | `Transport`, `TransportError`, `DiscordTransport` (moved out of `status.rs`) | 3 |
| `crack-core/src/messaging/courier.rs` (new) | `ReplySink`, `PoiseReplies`, `reply`, `reply_as`, `edit_reply`, `locate`, `Destination`, `post` | 3 |
| `crack-core/src/messaging/test_support.rs` (new, `cfg(test)`) | `FakeTransport`, `FakeReplies`, `Op`, `ReplyOp`, `description()` | 3 |
| `crack-core/src/messaging/status.rs` | uses `Transport` and `Rendered`; now-playing via `NowPlayingCard` | 3, 4 |
| `crack-core/src/messaging/track_failed.rs` | renders through `CrackedMessage::TrackFailed` | 5 |
| `crack-core/src/messaging/interface.rs` | builders shrink to card construction; embed formatting moves into `cards.rs` | 4, 7, 8 |
| `crack-core/src/music/remote.rs` | `Echo` → `messaging::cards::EchoLine`; announce via `Destination::Echo` | 5 |
| `crack-core/src/poise_ext.rs`, `crack-core/src/utils.rs` | send helpers become thin wrappers over `courier`, then are deleted in Task 10 | 5, 10 |
| `crack-core/src/config.rs` | `on_error` replies through `courier` | 5 |
| `crack-core/src/handlers/{track_end,idle}.rs` | `send_plain` and the idle alert via `post` | 5 |
| `crack-core/src/commands/music/*.rs`, `music/{query,queue}.rs`, `music/ops/{skip,edit}.rs`, `commands/music_utils.rs` | migrated call sites | 6–9 |
| `clippy.toml`; module headers of unmigrated modules | the ban | 10 |
| `Cargo.toml` (workspace version), `CHANGELOG.md`, `docs/` | release | 11 |

---

### Task 1: Formatting core (`messaging::format`)

**Files:**
- Create: `crack-core/src/messaging/format.rs`
- Modify: `crack-core/src/messaging/mod.rs` (add `pub mod format;`)
- Modify: `crack-core/src/music/audit_view.rs`: delete its `escape` and `cap` bodies and `pub(crate) use crate::messaging::format::{cap, escape};`. Keep `TITLE_MAX = 40`.
- Modify: `crack-core/src/commands/status.rs`: delete the duplicate `cap` and use `crate::messaging::format::{cap, INLINE_TITLE_MAX}` (its local `TITLE_MAX = 60` goes away).
- Modify: `crack-core/src/messaging/messages.rs`: add the constants below.
- Test: the `#[cfg(test)] mod tests` in `format.rs`

**Interfaces:**
- Consumes: `songbird::input::AuxMetadata`, `crate::music::audit::TrackRef { title: Option<String>, url: Option<String> }`.
- Produces (used by every later task):
  - `pub struct TrackLabel { pub title: Option<String>, pub url: Option<String>, pub duration: Option<Duration> }`
  - `TrackLabel::from_metadata(&AuxMetadata) -> TrackLabel`
  - `TrackLabel::from_ref(&TrackRef) -> TrackLabel`
  - `TrackLabel::title_text(&self, max: usize) -> String`
  - `TrackLabel::linked(&self, max: usize) -> String`
  - `pub fn cap(raw: &str, max: usize) -> String`
  - `pub fn escape(s: &str) -> String`
  - `pub fn clip(s: &str, max: usize) -> String`
  - `pub fn http_url(raw: Option<&str>) -> Option<url::Url>`
  - `pub fn duration_text(d: Option<Duration>) -> Option<String>`
  - `pub fn clock(d: Duration) -> String`
  - `pub enum Progress { Playing { position: Duration, duration: Option<Duration> }, Paused { position: Option<Duration> } }`
  - `pub fn progress_text(p: &Progress, now_unix: i64) -> String`
  - constants `INLINE_TITLE_MAX = 60`, `EMBED_TITLE_MAX = 256`, `CONTENT_MAX = 2000`, `DESCRIPTION_MAX = 4096`, `FIELD_MAX = 1024`, `AUTHOR_MAX = 256`

- [ ] **Step 1: Add the strings to `messages.rs`** (beside the `TRACK_FAILED_*` block):

```rust
pub const TRACK_UNTITLED: &str = "(untitled)";
pub const PROGRESS_ENDS: &str = "ends";
pub const PROGRESS_STARTED: &str = "Started";
pub const PROGRESS_PAUSED_AT: &str = "Paused at";
pub const PROGRESS_PAUSED: &str = "Paused";
```

Also make `TRACK_FAILED_UNTITLED` an alias: `pub const TRACK_FAILED_UNTITLED: &str = TRACK_UNTITLED;`.

- [ ] **Step 2: Write `format.rs` with failing tests first.** Create the file with the tests module below and stub bodies (`todo!()`), run, and see them fail:

```rust
//! The one place a track title, link, duration or URL becomes Discord text.
//!
//! Every message about a track goes through here, so an empty title, a
//! markdown character in a title, a relative URL or an unknown duration is
//! handled once. Before this, `SkipTo` printed "Skipped to **!" for a track
//! with an empty title, and `/play` printed "Track duration: 00:00".
use crate::messaging::messages::{
    PROGRESS_ENDS, PROGRESS_PAUSED, PROGRESS_PAUSED_AT, PROGRESS_STARTED, TRACK_UNTITLED,
};
use crate::music::audit::TrackRef;
use songbird::input::AuxMetadata;
use std::time::Duration;
use url::Url;

/// A title inside a sentence ("Skipped to **…**").
pub const INLINE_TITLE_MAX: usize = 60;
/// Discord's limits, in characters.
pub const EMBED_TITLE_MAX: usize = 256;
pub const CONTENT_MAX: usize = 2000;
pub const DESCRIPTION_MAX: usize = 4096;
pub const FIELD_MAX: usize = 1024;
pub const AUTHOR_MAX: usize = 256;

/// What a message knows about a track. Raw: everything is checked and
/// escaped when it is rendered, never before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackLabel {
    pub title: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
}

impl TrackLabel {
    #[must_use]
    pub fn from_metadata(meta: &AuxMetadata) -> Self {
        Self {
            title: meta.title.clone(),
            url: meta.source_url.clone(),
            duration: meta.duration,
        }
    }

    #[must_use]
    pub fn from_ref(track: &TrackRef) -> Self {
        Self {
            title: track.title.clone(),
            url: track.url.clone(),
            duration: None,
        }
    }

    /// The title for display: trimmed, `(untitled)` when blank, cut at `max`
    /// characters with `…`, then escaped -- it is third-party text.
    #[must_use]
    pub fn title_text(&self, max: usize) -> String {
        match self.title.as_deref().map(str::trim) {
            Some(t) if !t.is_empty() => escape(&cap(t, max)),
            _ => TRACK_UNTITLED.to_owned(),
        }
    }

    /// `[**title**](url)` when the URL is http(s), else `**title**`.
    #[must_use]
    pub fn linked(&self, max: usize) -> String {
        let title = self.title_text(max);
        match http_url(self.url.as_deref()) {
            Some(url) => format!("[**{title}**]({})", link_target(&url)),
            None => format!("**{title}**"),
        }
    }
}

/// A URL as a markdown link target: parentheses would end the link early.
fn link_target(url: &Url) -> String {
    url.as_str().replace('(', "%28").replace(')', "%29")
}

/// `raw` cut at `max` characters with `…`, or whole if it fits. Not escaped.
#[must_use]
pub fn cap(raw: &str, max: usize) -> String {
    let cut: String = raw.chars().take(max).collect();
    if raw.chars().count() > max {
        format!("{cut}…")
    } else {
        cut
    }
}

/// `s` cut so that it, `…` included, is at most `max` characters: Discord
/// rejects a message over its limits, and a trimmed one is better than none.
#[must_use]
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        cap(s, max.saturating_sub(1))
    }
}

/// Escape Discord markdown, links and mentions in text we did not write, and
/// flatten line breaks.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // A newline would split the one-line entry, and could split a page.
            '\n' | '\r' => out.push(' '),
            // `[`/`]` make masked links; `<` starts mentions and timestamps;
            // `@` starts @everyone/@here.
            '*' | '_' | '`' | '~' | '|' | '>' | '<' | '[' | ']' | '\\' | '@' => {
                out.push('\\');
                out.push(c);
            },
            _ => out.push(c),
        }
    }
    out
}

/// `raw` as an absolute http(s) URL, or `None`. Relative paths, empty
/// strings and other schemes (`javascript:`, `file:`) are all `None`.
#[must_use]
pub fn http_url(raw: Option<&str>) -> Option<Url> {
    let url = Url::parse(raw?.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// `m:ss`, or `h:mm:ss` from an hour up.
#[must_use]
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A track's length for display, or `None` when it is unknown or rounds to
/// zero -- never `0:00`.
#[must_use]
pub fn duration_text(d: Option<Duration>) -> Option<String> {
    let d = d?;
    (d.as_secs() > 0).then(|| clock(d))
}

/// Where a track is, for the now-playing progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Playing {
        position: Duration,
        duration: Option<Duration>,
    },
    /// `position` is `None` when the track would not say (see `get_info`).
    Paused { position: Option<Duration> },
}

/// The progress line. Playing with a known length: `4:33 · ends <t:…:R>`,
/// which Discord counts down by itself. Without one (a live stream):
/// `Started <t:…:R>`. Paused: `Paused at 1:12`, or just `Paused`.
#[must_use]
pub fn progress_text(p: &Progress, now_unix: i64) -> String {
    match *p {
        Progress::Playing {
            position,
            duration: Some(d),
        } if d.as_secs() > 0 => {
            let remaining = d.saturating_sub(position).as_secs() as i64;
            format!("{} · {PROGRESS_ENDS} <t:{}:R>", clock(d), now_unix + remaining)
        },
        Progress::Playing { position, .. } => {
            format!("{PROGRESS_STARTED} <t:{}:R>", now_unix - position.as_secs() as i64)
        },
        Progress::Paused {
            position: Some(position),
        } => format!("{PROGRESS_PAUSED_AT} {}", clock(position)),
        Progress::Paused { position: None } => PROGRESS_PAUSED.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(title: Option<&str>, url: Option<&str>) -> TrackLabel {
        TrackLabel {
            title: title.map(str::to_owned),
            url: url.map(str::to_owned),
            duration: None,
        }
    }

    #[test]
    fn a_blank_or_missing_title_is_untitled() {
        for t in [None, Some(""), Some("   "), Some("\n")] {
            assert_eq!(label(t, None).title_text(60), "(untitled)", "{t:?}");
        }
    }

    #[test]
    fn a_title_is_trimmed_escaped_and_cut() {
        assert_eq!(label(Some("  Want You Bad "), None).title_text(60), "Want You Bad");
        assert_eq!(
            label(Some("**[x](y)** @everyone"), None).title_text(60),
            "\\*\\*\\[x\\](y)\\*\\* \\@everyone"
        );
        assert_eq!(label(Some("abcdef"), None).title_text(3), "abc…");
        // A multibyte character straddling the cut must not panic.
        assert_eq!(label(Some("ééééé"), None).title_text(2), "éé…");
    }

    #[test]
    fn only_an_http_url_makes_a_link() {
        let t = Some("Song");
        assert_eq!(
            label(t, Some("https://youtu.be/x")).linked(60),
            "[**Song**](https://youtu.be/x)"
        );
        for bad in [None, Some(""), Some("/watch?v=x"), Some("javascript:alert(1)"), Some("file:///etc/passwd")] {
            assert_eq!(label(t, bad).linked(60), "**Song**", "{bad:?}");
        }
        assert_eq!(
            label(t, Some("https://en.wikipedia.org/wiki/Foo_(band)")).linked(60),
            "[**Song**](https://en.wikipedia.org/wiki/Foo_%28band%29)"
        );
    }

    #[test]
    fn durations_are_m_ss_then_h_mm_ss_and_never_zero() {
        assert_eq!(duration_text(None), None);
        assert_eq!(duration_text(Some(Duration::ZERO)), None);
        assert_eq!(duration_text(Some(Duration::from_millis(900))), None);
        assert_eq!(duration_text(Some(Duration::from_secs(5))).as_deref(), Some("0:05"));
        assert_eq!(duration_text(Some(Duration::from_secs(273))).as_deref(), Some("4:33"));
        assert_eq!(duration_text(Some(Duration::from_secs(3599))).as_deref(), Some("59:59"));
        assert_eq!(duration_text(Some(Duration::from_secs(3600))).as_deref(), Some("1:00:00"));
    }

    #[test]
    fn clip_keeps_the_result_within_the_limit() {
        assert_eq!(clip("hello", 5), "hello");
        assert_eq!(clip("hello!", 5), "hell…");
        assert_eq!(clip("hello!", 5).chars().count(), 5);
    }

    #[test]
    fn the_progress_line_in_each_state() {
        let now = 1_000_000;
        let playing = Progress::Playing {
            position: Duration::from_secs(73),
            duration: Some(Duration::from_secs(273)),
        };
        assert_eq!(progress_text(&playing, now), "4:33 · ends <t:1000200:R>");
        let live = Progress::Playing {
            position: Duration::from_secs(60),
            duration: None,
        };
        assert_eq!(progress_text(&live, now), "Started <t:999940:R>");
        let paused = Progress::Paused {
            position: Some(Duration::from_secs(72)),
        };
        assert_eq!(progress_text(&paused, now), "Paused at 1:12");
        assert_eq!(progress_text(&Progress::Paused { position: None }, now), "Paused");
    }

    #[test]
    fn a_label_reads_metadata_and_audit_refs() {
        let meta = AuxMetadata {
            title: Some("T".into()),
            source_url: Some("https://x.y/z".into()),
            duration: Some(Duration::from_secs(9)),
            ..Default::default()
        };
        assert_eq!(
            TrackLabel::from_metadata(&meta),
            TrackLabel {
                title: Some("T".into()),
                url: Some("https://x.y/z".into()),
                duration: Some(Duration::from_secs(9)),
            }
        );
        let r = TrackRef {
            title: Some("R".into()),
            url: None,
        };
        assert_eq!(TrackLabel::from_ref(&r), label(Some("R"), None));
    }
}
```

Note: `escape` gains `@` compared to `audit_view::escape`. Check `audit_view`'s and `remote`'s existing tests; update any literal that now gains a `\@`, and say so in the commit.

- [ ] **Step 3: Run the tests and confirm they fail.** Run `cargo test -p crack-core --lib messaging::format`. Expected: FAIL (panics at `todo!()`).
- [ ] **Step 4: Fill in the bodies as written above,** then run `cargo test -p crack-core --lib -- messaging::format audit_view commands::status remote`. Expected: all PASS.
- [ ] **Step 5: Sabotage each test once.** Break each in turn, see the named test fail, then restore:
  - drop the `.trim()`;
  - remove `'@'` from `escape`;
  - make `http_url` accept any scheme;
  - change `>= 3600` to `> 3600`;
  - make `clip` call `cap(s, max)`;
  - swap `+ remaining` for `- remaining`.
- [ ] **Step 6: Commit.**

```bash
git add crack-core/src/messaging/format.rs crack-core/src/messaging/mod.rs crack-core/src/messaging/messages.rs crack-core/src/music/audit_view.rs crack-core/src/commands/status.rs
git commit -m "messaging::format: one place for titles, links, durations and progress

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: `Rendered`, `render`, and the vocabulary changes

**Files:**
- Create: `crack-core/src/messaging/render.rs`
- Create: `crack-core/src/messaging/cards.rs` (types only in this task; renderers arrive in Tasks 4, 5 and 7)
- Modify: `crack-core/src/messaging/message.rs`:
  - `SkipTo { title, url }` becomes `SkipTo(TrackLabel)`;
  - `SongQueued { title, url }` becomes `SongQueued(TrackLabel)`;
  - their `Display` arms;
  - add `pub fn style(&self) -> Style`.
- Modify: `crack-core/src/music/ops/skip.rs` (`message()` builds `SkipTo(TrackLabel::from_ref(t))`; fix its test at line ~254)
- Modify: `crack-core/src/utils.rs:231` (`SongQueued(TrackLabel { title: Some(..), url: Some(..), duration: None })`)
- Modify: `crack-core/src/messaging/mod.rs` (`pub mod render; pub mod cards;`)

**Interfaces:**
- Consumes: Task 1's `format::*`.
- Produces:
  - `pub enum Mentions { #[default] None, Users }`
  - `pub struct Rendered { pub content: Option<String>, pub embed: Option<CreateEmbed<'static>>, pub components: Vec<CreateComponent<'static>>, pub mentions: Mentions }`
  - constructors `Rendered::text(impl Into<String>)`, `Rendered::embed(CreateEmbed<'static>)`, and `with_content`, `with_components`
  - conversions `to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static>`, `to_message(&self) -> CreateMessage<'static>`, `to_edit(&self) -> EditMessage<'static>`, `allowed_mentions(&self) -> CreateAllowedMentions<'static>`
  - `pub struct RenderCx { pub now_unix: i64, pub embed_links: bool }`, with `RenderCx::now()` and `Default` (now and `true`)
  - `pub enum Style { Embed, Text }`
  - `pub fn render(msg: &CrackedMessage, cx: &RenderCx) -> Rendered`
  - `#[cfg(test)] pub fn description(&Rendered) -> Option<String>`
  - in `cards.rs`:
    - `pub struct NowPlayingCard { pub label: TrackLabel, pub thumbnail: Option<String>, pub requester: Option<UserId>, pub progress: Progress }`
    - `pub struct QueuedCard { pub author: &'static str, pub label: TrackLabel, pub thumbnail: Option<String>, pub wait: Option<Duration> }`
    - `pub struct EchoLine { pub echo: Echo, pub user: UserId, pub via: Via }`
    - `pub enum Echo { Skipped { title: Option<String> }, Paused, Resumed, Repeat { on: bool }, Removed { title: Option<String> }, Shuffled }` (moved from `music::remote`; `remote` re-exports it as `pub use crate::messaging::cards::Echo;`)
    - `pub enum Via { Dashboard }` (PR 2 adds `Button`)
  - new `CrackedMessage` variants: `NowPlayingCard(Box<NowPlayingCard>)`, `Finished`, `TrackFailed { listed: Vec<track_failed::Failure>, more: usize }`, `Echo(Box<EchoLine>)`, `Queued(Box<QueuedCard>)`. They are appended **at the end of the enum**, so the existing discriminants (used by `PartialEq`) stay unchanged.

- [ ] **Step 1: Write the failing tests in `render.rs`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::TrackLabel;
    use crate::messaging::message::CrackedMessage;

    fn cx() -> RenderCx {
        RenderCx { now_unix: 0, embed_links: true }
    }

    /// The owner's TuneTitan screenshot, 2026-10-06: "⏭️ Skipped to **!".
    #[test]
    fn skipping_to_an_untitled_track_names_it_untitled() {
        let r = render(&CrackedMessage::SkipTo(TrackLabel::default()), &cx());
        assert_eq!(description(&r).as_deref(), Some("⏭️ Skipped to **(untitled)**!"));
    }

    #[test]
    fn skipping_to_a_titled_track_links_it_and_escapes_the_title() {
        let label = TrackLabel {
            title: Some("A*B".into()),
            url: Some("https://youtu.be/x".into()),
            duration: None,
        };
        let r = render(&CrackedMessage::SkipTo(label), &cx());
        assert_eq!(
            description(&r).as_deref(),
            Some("⏭️ Skipped to [**A\\*B**](https://youtu.be/x)!")
        );
    }

    #[test]
    fn nothing_rendered_may_ping_by_default() {
        let r = render(&CrackedMessage::Other("@everyone".into()), &cx());
        assert_eq!(r.mentions, Mentions::None);
        let am = serde_json::to_value(r.allowed_mentions()).unwrap();
        assert_eq!(am["parse"], serde_json::json!([]));
    }

    #[test]
    fn an_overlong_message_is_clipped_to_discords_limit() {
        let long = "x".repeat(5000);
        let r = render(&CrackedMessage::Other(long), &cx());
        assert_eq!(description(&r).unwrap().chars().count(), 4096);
    }

    #[test]
    fn a_text_style_variant_renders_as_content() {
        // `Pong` is the canary for Style::Text; see `CrackedMessage::style`.
        let r = render(&CrackedMessage::Pong, &cx());
        assert!(r.embed.is_none());
        assert!(r.content.is_some());
    }

    #[test]
    fn errors_are_red() {
        let r = render(&CrackedMessage::Error, &cx());
        let v = serde_json::to_value(r.embed.unwrap()).unwrap();
        assert_eq!(v["color"], serde_json::json!(serenity::all::Colour::RED.0));
    }
}
```

(Reading serenity's serialized `CreateEmbed` and `CreateAllowedMentions` as a `Value` in a test is fine: it is a third-party shape.)

- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p crack-core --lib messaging::render`. Expected: compile errors, then failures once it compiles.

- [ ] **Step 3: Implement `render.rs`:**

```rust
//! What a [`CrackedMessage`] looks like in Discord.
//!
//! `render` is pure and total: every message renders, nothing here touches
//! the network, and it is the only sender-facing way to turn a message into
//! Discord output. Titles, links, durations and limits come from
//! [`super::format`]; nothing else formats them.
use crate::messaging::format::{clip, CONTENT_MAX, DESCRIPTION_MAX};
use crate::messaging::message::CrackedMessage;
use serenity::all::{
    Colour, CreateAllowedMentions, CreateComponent, CreateEmbed, CreateMessage, EditMessage,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Who a message may ping. Every send states it; the default is nobody.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mentions {
    #[default]
    None,
    /// Users mentioned in the text may be pinged. Unused until the welcome
    /// message moves here.
    Users,
}

/// One message, ready to send.
#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub content: Option<String>,
    pub embed: Option<CreateEmbed<'static>>,
    pub components: Vec<CreateComponent<'static>>,
    pub mentions: Mentions,
}

impl Rendered {
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(clip(&content.into(), CONTENT_MAX)),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn embed(embed: CreateEmbed<'static>) -> Self {
        Self {
            embed: Some(embed),
            ..Self::default()
        }
    }

    /// Text that rides outside the embed: it survives a channel without
    /// `EMBED_LINKS`, where Discord strips the embed.
    #[must_use]
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(clip(&content.into(), CONTENT_MAX));
        self
    }

    #[must_use]
    pub fn with_components(mut self, components: Vec<CreateComponent<'static>>) -> Self {
        self.components = components;
        self
    }

    #[must_use]
    pub fn allowed_mentions(&self) -> CreateAllowedMentions<'static> {
        match self.mentions {
            Mentions::None => CreateAllowedMentions::new(),
            Mentions::Users => CreateAllowedMentions::new().all_users(true),
        }
    }

    #[must_use]
    pub fn to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static> {
        let mut reply = poise::CreateReply::default()
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            reply = reply.content(content.clone());
        }
        if let Some(embed) = &self.embed {
            reply = reply.embed(embed.clone());
        }
        reply
    }

    #[must_use]
    pub fn to_message(&self) -> CreateMessage<'static> {
        let mut m = CreateMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            m = m.content(content.clone());
        }
        if let Some(embed) = &self.embed {
            m = m.embed(embed.clone());
        }
        m
    }

    /// An edit replaces everything: a field left `None` is cleared, so a
    /// status that loses its buttons really loses them.
    #[must_use]
    pub fn to_edit(&self) -> EditMessage<'static> {
        let mut e = EditMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone())
            .content(self.content.clone().unwrap_or_default());
        e = match &self.embed {
            Some(embed) => e.embed(embed.clone()),
            None => e.embeds(Vec::new()),
        };
        e
    }
}

/// What rendering depends on that a message does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCx {
    /// Unix seconds, for Discord timestamps.
    pub now_unix: i64,
    /// Whether the channel shows embeds (`EMBED_LINKS`).
    pub embed_links: bool,
}

impl RenderCx {
    #[must_use]
    pub fn now() -> Self {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or_default();
        Self {
            now_unix,
            embed_links: true,
        }
    }
}

impl Default for RenderCx {
    fn default() -> Self {
        Self::now()
    }
}

/// Whether a message is an embed or plain text. Decided per variant, once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Embed,
    Text,
}

/// Render `msg`. Pure and total.
#[must_use]
pub fn render(msg: &CrackedMessage, cx: &RenderCx) -> Rendered {
    match msg {
        CrackedMessage::CreateEmbed(embed) => Rendered::embed(*embed.clone()),
        CrackedMessage::NowPlayingCard(card) => super::cards::now_playing(card, cx),
        CrackedMessage::Finished => super::cards::finished(),
        CrackedMessage::Queued(card) => super::cards::queued(card, cx),
        CrackedMessage::Echo(line) => super::cards::echo(line),
        CrackedMessage::TrackFailed { listed, more } => Rendered::embed(
            CreateEmbed::new().description(clip(
                &super::track_failed::render_text(listed, *more),
                DESCRIPTION_MAX,
            )),
        ),
        other => {
            let text = other.to_string();
            match other.style() {
                Style::Text => Rendered::text(text),
                Style::Embed => Rendered::embed(
                    CreateEmbed::new()
                        .description(clip(&text, DESCRIPTION_MAX))
                        .colour(Colour::from(other)),
                ),
            }
        },
    }
}

/// The embed description of `r`, for tests.
#[cfg(test)]
#[must_use]
pub fn description(r: &Rendered) -> Option<String> {
    let embed = r.embed.as_ref()?;
    let v = serde_json::to_value(embed).ok()?;
    v["description"].as_str().map(str::to_owned)
}
```

The card renderers `now_playing`, `finished`, `queued` and `echo` are stubbed in `cards.rs` in this task:
- `finished()` returns `Rendered::embed(crate::messaging::status::finished_embed())`;
- `echo(line)` returns `Rendered::embed(CreateEmbed::new().description(line.line()))`, with `EchoLine::line` moved verbatim from `remote::Echo::line` but using `format::{cap, escape}` and `INLINE_TITLE_MAX`;
- `now_playing` and `queued` return `Rendered::default()` until Tasks 4 and 7.

`track_failed::render_text` is today's `track_failed::render`, renamed. Keep a `pub fn render(listed, more) -> String` alias if any test still uses it; Task 5 removes it.

- [ ] **Step 4: Add `style()` to `CrackedMessage`** in `message.rs`:

```rust
impl CrackedMessage {
    /// Embed or plain text. Everything is an embed except the few replies
    /// that have always been text: a ping, and command output meant to be
    /// copied.
    #[must_use]
    pub fn style(&self) -> crate::messaging::render::Style {
        use crate::messaging::render::Style;
        match self {
            Self::Pong | Self::Version { .. } | Self::Prefixes(_) => Style::Text,
            _ => Style::Embed,
        }
    }
}
```

Before committing, check each `Text` variant against its call site. If its call site today sends it as an embed (`send_reply(.., true)`), drop it from the `Text` arm, and keep `Pong` only if `ping.rs` sends text. The canary test must name a variant that really is text. If none is, change `a_text_style_variant_renders_as_content` to build `Rendered::text` directly and remove the `Text` arm, leaving `Style::Text` for Task 9's `diagnose`/`vote` text replies.

- [ ] **Step 5: Change the two track variants.** Make them `SkipTo(TrackLabel)` and `SongQueued(TrackLabel)` (declared in their **original enum positions**), and change their `Display` arms to:

```rust
Self::SongQueued(label) => f.write_str(&format!("{} {}", ADDED_QUEUE, label.linked(INLINE_TITLE_MAX))),
Self::SkipTo(label) => f.write_str(&format!("{} {}!", SKIPPED_TO, label.linked(INLINE_TITLE_MAX))),
```

Update `ops/skip.rs::message()` to `Some(t) => CrackedMessage::SkipTo(TrackLabel::from_ref(t))`, and its test's pattern to `CrackedMessage::SkipTo(_)`. Update `utils.rs:231`.

- [ ] **Step 6: Append the five new variants at the end of the enum.** Give them `Display` arms that return their plain text, for logs only:
  - `NowPlayingCard` → `QUEUE_NOW_PLAYING`
  - `Finished` → `STATUS_FINISHED_TITLE`
  - `TrackFailed` → `track_failed::render_text(listed, *more)`
  - `Echo` → `line.line()`
  - `Queued` → `card.author`
- [ ] **Step 7: Run the tests.** Run `cargo test -p crack-core --lib -- messaging ops::skip`. Expected: PASS.
- [ ] **Step 8: Sabotage, then commit.** Sabotage each test once:
  - remove `.colour(..)`;
  - drop the `clip`;
  - map `Mentions::None` to `all_users(true)`;
  - make `SkipTo`'s `Display` use `label.title.clone().unwrap_or_default()`.

  Then run `cargo +nightly fmt --all`, `cargo clippy -p crack-core --all-targets`, and commit the touched paths with message "messaging::render: one renderer, track variants carry a TrackLabel".

---

### Task 3: `Transport`, `ReplySink`, the courier, and the fakes

**Files:**
- Create: `crack-core/src/messaging/transport.rs`. Move `TransportError`, `is_unknown_message` usage, `StatusTransport` (renamed `Transport`) and `DiscordTransport` here from `status.rs`. Leave `pub use super::transport::{Transport, TransportError, DiscordTransport};` in `status.rs` so existing paths keep compiling during the migration.
- Create: `crack-core/src/messaging/courier.rs`
- Create: `crack-core/src/messaging/test_support.rs` (`#[cfg(test)] pub(crate) mod test_support;` in `mod.rs`)
- Modify: `crack-core/src/messaging/status.rs`:
  - `apply`, `apply_after`, `update`, `update_after` and `announce` take `Rendered` instead of `CreateEmbed<'static>`;
  - its test `Fake` is replaced by `test_support::FakeTransport`;
  - the `Op` assertions keep their shape: `Send(channel)`, `Edit(channel, id)`, `Delete(channel, id)`.
- Modify: `crack-core/src/messaging/track_failed.rs`: its test `Fake` is replaced by `FakeTransport`, and `report` passes `Rendered`.
- Modify: `crack-core/src/music/remote.rs` and `crack-core/src/handlers/track_end.rs`: change their constructions of `DiscordTransport` and calls into `status` to pass `Rendered`.

**Interfaces:**
- Consumes: Task 2's `Rendered`, `render`, `RenderCx`.
- Produces:

```rust
// transport.rs
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, channel: GenericChannelId, out: Rendered) -> Result<MessageId, TransportError>;
    async fn edit(&self, channel: GenericChannelId, id: MessageId, out: Rendered) -> Result<(), TransportError>;
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>;
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId>;
}
pub struct DiscordTransport { pub http: Arc<Http>, pub cache: Arc<Cache> }

// courier.rs
#[async_trait]
pub trait ReplySink: Send + Sync {
    type Handle: Send + Sync;
    async fn send(&self, out: Rendered, ephemeral: bool) -> Result<Self::Handle, CrackedError>;
    async fn edit(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError>;
    /// The reply as a channel message, `None` for an ephemeral one or when
    /// it cannot be read.
    async fn locate(&self, handle: &Self::Handle) -> Option<(GenericChannelId, MessageId)>;
}
pub struct PoiseReplies<'ctx>(pub crate::Context<'ctx>);   // Handle = poise::ReplyHandle<'ctx>

pub async fn reply_on<S: ReplySink>(sink: &S, msg: &CrackedMessage, cx: &RenderCx, ephemeral: bool) -> Result<S::Handle, CrackedError>;
pub async fn edit_reply_on<S: ReplySink>(sink: &S, handle: &S::Handle, msg: &CrackedMessage, cx: &RenderCx) -> Result<(), CrackedError>;
pub async fn locate_on<S: ReplySink>(sink: &S, handle: &S::Handle) -> Option<(GenericChannelId, MessageId)>;
/// `locate_on` over poise: replaces `status::reply_floor`.
pub async fn locate<'ctx>(ctx: crate::Context<'ctx>, handle: &poise::ReplyHandle<'ctx>) -> Option<(GenericChannelId, MessageId)>;

// Command-facing conveniences over PoiseReplies:
pub async fn reply<'ctx>(ctx: crate::Context<'ctx>, msg: CrackedMessage) -> Result<poise::ReplyHandle<'ctx>, CrackedError>;
pub async fn reply_as<'ctx>(ctx: crate::Context<'ctx>, msg: CrackedMessage, ephemeral: bool) -> Result<poise::ReplyHandle<'ctx>, CrackedError>;
pub async fn edit_reply<'ctx>(ctx: crate::Context<'ctx>, handle: &poise::ReplyHandle<'ctx>, msg: CrackedMessage) -> Result<(), CrackedError>;
/// A reply that is already rendered (the degraded EMBED_LINKS case builds
/// content + embed together).
pub async fn reply_rendered<'ctx>(ctx: crate::Context<'ctx>, out: Rendered, ephemeral: bool) -> Result<poise::ReplyHandle<'ctx>, CrackedError>;
pub async fn edit_rendered<'ctx>(ctx: crate::Context<'ctx>, handle: &poise::ReplyHandle<'ctx>, out: Rendered) -> Result<(), CrackedError>;

pub enum Destination {
    Channel(GenericChannelId),
    /// The floating status message; `after` is a visible reply it must land below.
    Status { guild: GuildId, after: Option<(GenericChannelId, MessageId)> },
    /// Where Status would land; the tracked status message is left alone.
    Echo(GuildId),
}
/// Deliver `msg` to `dest`. Best effort: failures are logged and swallowed,
/// and the result says where it landed, if anywhere.
pub async fn post(data: &Data, transport: &dyn Transport, dest: Destination, msg: &CrackedMessage, cx: &RenderCx) -> Option<(GenericChannelId, MessageId)>;
```

`post` mappings:
- `Channel(id)`: `transport.send(id, render(msg, cx))`. On `Err`, `tracing::warn!` and `None`.
- `Status { guild, after }`: `status::update_after(data, transport, guild, render(msg, cx), phase, after)`. `phase` is `Phase::Finished` for `CrackedMessage::Finished` and `Phase::Playing` otherwise. Returns the shown `(channel, id)`.
- `Echo(guild)`: `status::announce(data, transport, guild, render(msg, cx))`.

The `PoiseReplies` impl:
- `send` → `self.0.send(out.to_reply(ephemeral)).await.map_err(Into::into)`
- `edit` → `handle.edit(self.0, out.to_reply(false)).await.map_err(Into::into)`. This is #494: `ReplyHandle::edit` routes the response and followups correctly.
- `locate` → `handle.message().await.ok().map(|m| (m.channel_id, m.id))`. This replaces `status::reply_floor`, which becomes `courier::locate(ctx, &handle)`.

- [ ] **Step 1: Write `test_support.rs`:**

```rust
//! Stand-in Discord for messaging tests: everything sent is recorded.
use super::courier::ReplySink;
use super::render::{description, Rendered};
use super::transport::{Transport, TransportError};
use crate::errors::CrackedError;
use async_trait::async_trait;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Send(u64),
    Edit(u64, u64),
    Delete(u64, u64),
}

/// Records every call, keeps every rendered message, answers from what the
/// test set.
#[derive(Default)]
pub struct FakeTransport {
    pub last: Mutex<Option<MessageId>>,
    pub edit_error: Mutex<Option<TransportError>>,
    pub delete_error: Mutex<Option<TransportError>>,
    pub send_error: Mutex<Option<TransportError>>,
    pub ops: Mutex<Vec<Op>>,
    pub sent: Mutex<Vec<Rendered>>,
    next: AtomicU64,
}

impl FakeTransport {
    #[must_use]
    pub fn with_last(self, last: u64) -> Self {
        *self.last.lock().unwrap() = Some(MessageId::new(last));
        self
    }
    pub fn ops(&self) -> Vec<Op> {
        self.ops.lock().unwrap().clone()
    }
    /// The embed descriptions (else content) of everything sent or edited, in order.
    pub fn texts(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|r| description(r).or_else(|| r.content.clone()).unwrap_or_default())
            .collect()
    }
}

#[async_trait]
impl Transport for FakeTransport {
    async fn send(&self, channel: GenericChannelId, out: Rendered) -> Result<MessageId, TransportError> {
        self.ops.lock().unwrap().push(Op::Send(channel.get()));
        self.sent.lock().unwrap().push(out);
        if let Some(err) = self.send_error.lock().unwrap().clone() {
            return Err(err);
        }
        Ok(MessageId::new(1000 + self.next.fetch_add(1, Ordering::SeqCst)))
    }
    async fn edit(&self, channel: GenericChannelId, id: MessageId, out: Rendered) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(Op::Edit(channel.get(), id.get()));
        self.sent.lock().unwrap().push(out);
        match self.edit_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(Op::Delete(channel.get(), id.get()));
        match self.delete_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn last_message_id(&self, _guild: GuildId, _channel: GenericChannelId) -> Option<MessageId> {
        *self.last.lock().unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyOp {
    Send { ephemeral: bool, text: String },
    EditHandle { handle: u64, text: String },
}

/// A stand-in for poise's reply path. Handles are numbered from 1.
#[derive(Default)]
pub struct FakeReplies {
    pub ops: Mutex<Vec<ReplyOp>>,
    pub sent: Mutex<Vec<Rendered>>,
    next: AtomicU64,
}

impl FakeReplies {
    pub fn ops(&self) -> Vec<ReplyOp> {
        self.ops.lock().unwrap().clone()
    }
}

fn text_of(r: &Rendered) -> String {
    description(r).or_else(|| r.content.clone()).unwrap_or_default()
}

#[async_trait]
impl ReplySink for FakeReplies {
    type Handle = u64;
    async fn send(&self, out: Rendered, ephemeral: bool) -> Result<u64, CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::Send { ephemeral, text: text_of(&out) });
        self.sent.lock().unwrap().push(out);
        Ok(1 + self.next.fetch_add(1, Ordering::SeqCst))
    }
    async fn edit(&self, handle: &u64, out: Rendered) -> Result<(), CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::EditHandle { handle: *handle, text: text_of(&out) });
        self.sent.lock().unwrap().push(out);
        Ok(())
    }
    async fn locate(&self, handle: &u64) -> Option<(GenericChannelId, MessageId)> {
        Some((GenericChannelId::new(5), MessageId::new(*handle)))
    }
}
```

- [ ] **Step 2: Write the failing courier tests** in `courier.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::TrackLabel;
    use crate::messaging::test_support::{FakeReplies, FakeTransport, Op, ReplyOp};
    use crate::{Data, DataInner};
    use std::sync::Arc;

    fn cx() -> RenderCx {
        RenderCx { now_unix: 0, embed_links: true }
    }

    #[tokio::test]
    async fn a_reply_is_rendered_and_sent_with_its_privacy() {
        let sink = FakeReplies::default();
        reply_on(&sink, &CrackedMessage::Skip, &cx(), true).await.unwrap();
        assert_eq!(
            sink.ops(),
            vec![ReplyOp::Send { ephemeral: true, text: "⏭️ Skipped!".into() }]
        );
    }

    /// #494: an edit goes to the handle it was given, never `@original`.
    #[tokio::test]
    async fn an_edit_targets_the_handle_it_was_given() {
        let sink = FakeReplies::default();
        let first = reply_on(&sink, &CrackedMessage::Search, &cx(), false).await.unwrap();
        let second = reply_on(&sink, &CrackedMessage::Search, &cx(), false).await.unwrap();
        edit_reply_on(&sink, &second, &CrackedMessage::SkipTo(TrackLabel::default()), &cx())
            .await
            .unwrap();
        assert_eq!(first, 1);
        assert!(matches!(sink.ops()[2], ReplyOp::EditHandle { handle: 2, .. }));
    }

    /// The status floor: a visible reply's place is what the next status
    /// update lands below (`status::update_after`'s `after`).
    #[tokio::test]
    async fn a_visible_reply_says_where_it_landed() {
        let sink = FakeReplies::default();
        let h = reply_on(&sink, &CrackedMessage::Skip, &cx(), false).await.unwrap();
        assert_eq!(
            locate_on(&sink, &h).await,
            Some((GenericChannelId::new(5), MessageId::new(1)))
        );
    }

    #[tokio::test]
    async fn posting_to_a_channel_sends_once_and_says_where() {
        let data = Data(Arc::new(DataInner::default()));
        let t = FakeTransport::default();
        let at = post(&data, &t, Destination::Channel(GenericChannelId::new(7)), &CrackedMessage::Clear, &cx()).await;
        assert_eq!(t.ops(), vec![Op::Send(7)]);
        assert_eq!(at, Some((GenericChannelId::new(7), MessageId::new(1000))));
    }

    #[tokio::test]
    async fn a_failed_channel_post_is_swallowed() {
        let data = Data(Arc::new(DataInner::default()));
        let t = FakeTransport::default();
        *t.send_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        let at = post(&data, &t, Destination::Channel(GenericChannelId::new(7)), &CrackedMessage::Clear, &cx()).await;
        assert_eq!(at, None);
    }
}
```

The literal `"⏭️ Skipped!"` must equal `SKIPPED` in `messages.rs`. Check it; if it differs, pin the real wording. The test is meant to pin today's wording, not to change it.

- [ ] **Step 3: Run them and see them fail.** Run `cargo test -p crack-core --lib messaging::courier`. Expected: FAIL.
- [ ] **Step 4: Implement `transport.rs` and `courier.rs`** as specified in Interfaces. `DiscordTransport` uses `out.to_message()` for send and `out.to_edit()` for edit. Move `is_unknown_message` with it, if it lives in `status.rs`; otherwise keep the import.
- [ ] **Step 5: Convert `status.rs` and `track_failed.rs`** to `Transport` and `Rendered`:
  - `status::show_now_playing_after` and `show_finished` pass `Rendered::embed(..)` for now; Task 4 swaps in the card.
  - Replace both test fakes with `FakeTransport`. The existing assertions on `Op`s carry over unchanged, and assertions on text use `fake.texts()`.
  - Every existing test in `status.rs` and `track_failed.rs` must still pass, unchanged in meaning.
- [ ] **Step 6: Run the tests.** Run `cargo test -p crack-core --lib -- messaging music::remote handlers::track_end`. Expected: PASS, with the same count as before plus the 4 new courier tests.
- [ ] **Step 7: Sabotage, then commit.** Sabotage each new test once:
  - ignore `ephemeral`;
  - make `edit_reply_on` edit handle 1;
  - return `Some` on a send error.

  Then fmt, clippy, and commit with message "messaging: one Transport, a ReplySink, and the courier".

---

### Task 4: The now-playing card, the finished card, and the live progress line

**Files:**
- Modify: `crack-core/src/messaging/cards.rs`: implement `now_playing(card, cx)` and `finished()`.
- Modify: `crack-core/src/messaging/interface.rs`:
  - replace `build_now_playing_embed_metadata` and `create_now_playing_embed` with `pub async fn now_playing_card(track: &TrackHandle) -> NowPlayingCard`;
  - delete `send_now_playing` if nothing calls it (check with `rg send_now_playing`); otherwise route it through `post`.
- Modify: `crack-core/src/messaging/status.rs`:
  - `show_now_playing_after` builds `CrackedMessage::NowPlayingCard(Box::new(now_playing_card(&track).await))` and posts it with `Destination::Status { guild, after }` through `courier::post`;
  - `show_finished` posts `CrackedMessage::Finished` the same way.
- Modify: every other caller of `create_now_playing_embed` (`rg create_now_playing_embed`, for example `doplay.rs`). Each becomes `render(&CrackedMessage::NowPlayingCard(Box::new(now_playing_card(&t).await)), &RenderCx::now()).embed` until Task 7 migrates it fully.
- Test: `cards.rs` tests; `interface.rs` test for the `get_info` timeout.

**Interfaces:**
- Consumes: `format::{TrackLabel, Progress, progress_text, http_url, clip, EMBED_TITLE_MAX, FIELD_MAX, AUTHOR_MAX}`, `render::{Rendered, RenderCx}`, and `utils::{get_track_handle_metadata, get_requesting_user, build_footer_info}`. The vanity line is the third element of `build_footer_info`'s tuple; the footer text and icon come from the URL host.
- Produces: `cards::now_playing(&NowPlayingCard, &RenderCx) -> Rendered`, `cards::finished() -> Rendered`, `interface::now_playing_card(&TrackHandle) -> NowPlayingCard`.

**Now-playing embed layout.** This is today's layout, with the fixes:
- **author:** `QUEUE_NOW_PLAYING`, clipped to `AUTHOR_MAX`.
- **title:** `label.title_text(EMBED_TITLE_MAX)`.
- **url:** set only if `http_url(label.url)` is `Some`.
- **field `PROGRESS`** (inline): `">>> " + progress_text(&card.progress, cx.now_unix)`, clipped to `FIELD_MAX`.
- **field `REQUESTED_BY`** (inline): `">>> " + requesting_user_to_string(id)`, or `">>> N/A"` when `None`.
- **thumbnail:** only if `http_url(card.thumbnail)` is `Some`. No `url::Url::parse` error log.
- **description:** the vanity line from `build_footer_info`.
- **footer:** only when `http_url(label.url)` has a host: `"Streaming via {host}"` with the favicon icon, exactly as `build_footer_info` builds them for a valid URL. With no valid URL there is **no footer** (no "Streaming via unknown").
- **colour:** none, as today.

**Reading the card.** In `now_playing_card`:
- `get_info()` is wrapped in `tokio::time::timeout(crate::music::ops::TRACK_INFO_TIMEOUT, track.get_info())`.
- On `Ok(Ok(info))`: if `info.playing == PlayMode::Pause`, the progress is `Progress::Paused { position: Some(info.position) }`; otherwise `Progress::Playing { position: info.position, duration: label.duration }`.
- On timeout or error: `Progress::Playing { position: Duration::ZERO, duration: label.duration }`.

- [ ] **Step 1: Write the failing tests in `cards.rs`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::{Progress, TrackLabel};
    use crate::messaging::render::RenderCx;
    use std::time::Duration;

    fn v(r: &Rendered) -> serde_json::Value {
        serde_json::to_value(r.embed.as_ref().unwrap()).unwrap()
    }

    fn card(url: Option<&str>, thumb: Option<&str>) -> NowPlayingCard {
        NowPlayingCard {
            label: TrackLabel {
                title: Some("OH SHIT I'M FEELING IT".into()),
                url: url.map(str::to_owned),
                duration: Some(Duration::from_secs(273)),
            },
            thumbnail: thumb.map(str::to_owned),
            requester: Some(UserId::new(42)),
            progress: Progress::Playing { position: Duration::from_secs(73), duration: Some(Duration::from_secs(273)) },
        }
    }

    #[test]
    fn the_now_playing_card_shows_a_live_end_time() {
        let r = now_playing(&card(Some("https://www.youtube.com/watch?v=x"), Some("https://i.ytimg.com/a.jpg")), &RenderCx { now_unix: 1_000_000, embed_links: true });
        let e = v(&r);
        assert_eq!(e["title"], "OH SHIT I'M FEELING IT");
        assert_eq!(e["url"], "https://www.youtube.com/watch?v=x");
        assert_eq!(e["fields"][0]["value"], ">>> 4:33 · ends <t:1000200:R>");
        assert_eq!(e["fields"][1]["value"], ">>> <@42>");
        assert_eq!(e["thumbnail"]["url"], "https://i.ytimg.com/a.jpg");
        assert_eq!(e["footer"]["text"], "Streaming via youtube.com");
    }

    /// The members-only link on TuneTitan (2026-10-06): no title, no URL,
    /// no thumbnail. No `RelativeUrlWithoutBase`, no "Streaming via unknown".
    #[test]
    fn a_track_with_nothing_known_renders_cleanly() {
        let mut c = card(None, Some(""));
        c.label = TrackLabel::default();
        c.progress = Progress::Playing { position: Duration::ZERO, duration: None };
        let e = v(&now_playing(&c, &RenderCx { now_unix: 50, embed_links: true }));
        assert_eq!(e["title"], "(untitled)");
        assert!(e.get("url").is_none() || e["url"].is_null());
        assert!(e.get("thumbnail").is_none() || e["thumbnail"].is_null());
        assert!(e.get("footer").is_none() || e["footer"].is_null());
        assert_eq!(e["fields"][0]["value"], ">>> Started <t:50:R>");
    }

    #[test]
    fn a_paused_card_says_where_it_paused() {
        let mut c = card(None, None);
        c.progress = Progress::Paused { position: Some(Duration::from_secs(72)) };
        assert_eq!(v(&now_playing(&c, &RenderCx { now_unix: 0, embed_links: true }))["fields"][0]["value"], ">>> Paused at 1:12");
    }
}
```

The literal `"Streaming via youtube.com"` must equal what `build_footer_info` produces for that URL today. Run it first: if today's text is `www.youtube.com`, pin that, since wording stays unchanged.

- [ ] **Step 2: Write the failing timeout test** in `interface.rs`. It uses `crate::music::ops::test_support::queue_of(1)`, takes the track handle from `call.lock().await.queue().current_queue()[0]`, and wraps `now_playing_card(&handle)` in an outer `tokio::time::timeout(Duration::from_secs(5), ..)`. Assert it completes, that `card.label.title == Some("t0")`, and that `matches!(card.progress, Progress::Playing { position, .. } if position == Duration::ZERO)`. Use `#[tokio::test(start_paused = false)]`. `get_info` never answers offline, so this proves the bound.
- [ ] **Step 3: Run and see the failures.** Run `cargo test -p crack-core --lib -- messaging::cards messaging::interface`. Expected: FAIL.
- [ ] **Step 4: Implement** `cards::now_playing`, `cards::finished`, `interface::now_playing_card`; switch `status.rs` to `courier::post`; replace the other callers.
- [ ] **Step 5: Run the tests.** Run `cargo test -p crack-core --lib -- messaging music handlers`. Expected: PASS. All `status.rs` tests still pass.
- [ ] **Step 6: Sabotage, then commit.** Sabotage each test once:
  - drop the timeout;
  - always set the footer;
  - use `Url::parse` without the scheme check;
  - drop `+ remaining` in `format`.

  Commit with message "Now playing: a live end time, and nothing rendered from a missing URL".

---

### Task 5: The shared routes

**Files:**
- Modify: `crack-core/src/poise_ext.rs`:
  - `send_reply`, `send_reply_owned`, `send_reply_embed`, `send_message`, `send_message_owned` and `send_embed_response` keep their signatures, but their bodies call `courier`;
  - `send_message(params)` becomes `courier::reply_rendered(ctx, rendered, params.ephemeral)`, where `rendered` is `render(&params.msg, &RenderCx::now())`, or `Rendered::embed(params.embed)` when `params.embed` is `Some`;
  - `params.as_embed == false` maps to `Rendered::text(params.msg.to_string())`;
  - `params.color` is applied to the embed (`.colour(params.color)`) when set and not `Colour::BLUE`;
  - delete the `colored` ANSI branch.
- Modify: `crack-core/src/utils.rs`: `send_reply`, `send_reply_owned`, `send_nonembed_reply`, `send_embed_response_poise`, `send_embed_response_poise_as`, `edit_response_poise`, `edit_embed_response2`, `edit_embed_response_poise` route through `courier` with the same signatures. `edit_embed_response2(ctx, embed, handle, content)` becomes `courier::edit_rendered(ctx, &handle, Rendered::embed(embed) + content)`.
- Modify: `crack-core/src/config.rs`: both `on_error` arms reply with `courier::reply(ctx, CrackedMessage::CrackedError(err))`; `check_reply` handling stays.
- Modify: `crack-core/src/messaging/track_failed.rs`:
  - `report` renders `CrackedMessage::TrackFailed { listed: listed.clone(), more }` via `render`, instead of building its own embed;
  - remove the `render` alias left in Task 2 (`render_text` stays as the text builder);
  - the existing 17 tests keep passing.
- Modify: `crack-core/src/music/remote.rs`:
  - `Echo` and `Echo::line` are gone (moved in Task 2);
  - `control`'s spawned task calls `courier::post(&cx.data, &transport, Destination::Echo(guild_id), &CrackedMessage::Echo(Box::new(EchoLine { echo: posted, user, via: Via::Dashboard })), &RenderCx::now())`, and passes its result as the settle anchor;
  - `echo_embed` is deleted;
  - remote's existing echo tests now assert via `render(&CrackedMessage::Echo(..))`.
- Modify: `crack-core/src/handlers/track_end.rs`:
  - `send_plain(channel, http, content)` becomes `courier::post(&self.data, &DiscordTransport{..}, Destination::Channel(channel), &CrackedMessage::Other(content.to_owned()), &RenderCx::now())`. That makes the autoplay notices embeds, the same as every other notice; this is a deliberate wording-neutral style change, noted in the CHANGELOG.
  - `update_queue_messages`' `EditMessage` stays until Task 8.
- Modify: `crack-core/src/handlers/idle.rs:102`: the idle alert becomes `courier::post(.., Destination::Channel(self.channel_id), &CrackedMessage::Other(IDLE_ALERT.to_owned()), ..)`.

- [ ] **Step 1: Write failing tests.**
  - In `poise_ext.rs` (or `courier.rs` if `poise_ext` has no test module), test against `FakeReplies` that a `SendMessageParams { as_embed: false, .. }` reply arrives as content with **no ANSI escape** (`!text.contains('\u{1b}')`). Extract the params → `Rendered` mapping into a pure `fn rendered_from_params(params: &SendMessageParams) -> Rendered`, and test that function.
  - In `remote.rs`, the existing echo tests, now through `render`.
  - In `track_failed.rs`, the existing tests, with `fake.texts()`.
- [ ] **Step 2: Run them and see them fail.** Run `cargo test -p crack-core --lib -- poise_ext messaging music::remote`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the tests.** Run `cargo test --workspace`. Expected: PASS. crack-web compiles; it uses `remote::control`, whose signature is unchanged.
- [ ] **Step 5: Sabotage, then commit.** Sabotage `rendered_from_params` (put the ANSI back) and see the test fail. Commit with message "messaging: replies, errors, notices and echoes all go through the courier".

---

### Task 6: Transport commands

**Files (each site → its replacement):**

| Site | Today | After |
|---|---|---|
| `commands/music/skip.rs:46-68` `send_skip_reply` | builds `SendMessageParams` | `courier::reply_as(ctx, send_msg, private).await?` then `.into_message().await?` (keep returning `Message`) |
| `pause.rs:24`, `resume.rs:29`, `repeat.rs:38`, `stop.rs:26`, `clear.rs:34`, `leave.rs:38,43`, `shuffle.rs:34,51`, `seek.rs:60` | `send_reply(&ctx, msg, true)` | `courier::reply(ctx, msg).await?` |
| `volume.rs:55` | `send_embed_response_poise(ctx, embed)` | a `CrackedMessage::Volume { vol, old_vol }` reply via `courier::reply` **if** the hand-built embed's text equals `Volume`'s `Display`; otherwise keep the embed as `CrackedMessage::CreateEmbed(Box::new(embed))` via `courier::reply`, and record which in the report |
| `remove.rs:50,52` | `send_embed_response_poise(ctx, embed)`, `send_reply(.., RemoveMultiple, true)` | `courier::reply(ctx, CrackedMessage::CreateEmbed(Box::new(embed)))` and `courier::reply(ctx, CrackedMessage::RemoveMultiple)` |
| `music/ops/edit.rs:52-60` (the removed-track embed's title formatting) | `format!("[**{title}**]({url})")` with `QUEUE_NO_TITLE` | `TrackLabel::from_ref(first).linked(INLINE_TITLE_MAX)` |

- [ ] **Step 1: Pin the new wording in tests.** Write a test in `ops/edit.rs` pinning that removing an untitled track renders `"(untitled)"` (not `"Unknown title"`), and a titled one with an http URL renders `[**t**](url)`. Run it and see it fail.
- [ ] **Step 2: Apply the table.** Remove the now-unused imports.
- [ ] **Step 3: Run the tests.** Run `cargo test -p crack-core --lib` and `cargo clippy -p crack-core --all-targets`. Expected: PASS and clean.
- [ ] **Step 4: Sabotage, then commit.** Sabotage the edit test, then commit with message "Transport commands reply through the courier".

---

### Task 7: The `/play` path

**Files:** `commands/music/doplay.rs`, `commands/music/dosearch.rs`, `music/query.rs` (lines ~352 and ~610), `music/queue.rs` (playlist progress edits ~527 and ~619), `messaging/cards.rs` (`queued`), `messaging/interface.rs` (search builders), `utils.rs` (`yt_search_select`).

**Replace `build_queued_embed` with `CrackedMessage::Queued(Box<QueuedCard>)`:**
- `cards::queued(card, cx)` renders:
  - author: `card.author`;
  - title: `card.label.title_text(EMBED_TITLE_MAX)`;
  - url: only if it is http(s);
  - thumbnail: only if it is http(s);
  - footer: `TRACK_DURATION {d}` **only if** `duration_text(label.duration)` is `Some`, and `TRACK_TIME_TO_PLAY {w}` **only if** `duration_text(card.wait)` is `Some`, joined by `\n`; no footer if neither.
- `build_play_embed` builds the card from the track metadata (`TrackLabel::from_metadata`, `thumbnail`), with `wait` = `calculate_time_until_play(..)`, which already returns `Option<Duration>`. Pass `None` when any queued track's duration is unknown: check that function; if it treats an unknown duration as zero, change it to return `None` and test that.

**The degraded `EMBED_LINKS` notice:**
- `build_play_reply` returns a `Rendered`:
  - `NoticeDelivery::Field(note)` adds the field to the card's embed, after `render`;
  - `NoticeDelivery::Content(note)` sets `.with_content(note)`.
- The reply is sent or edited with `courier::edit_rendered(ctx, &search_msg, rendered)`.
- Keep the existing `degraded_delivery` tests passing.

**The other `doplay` sites:**
- `:52` `ctx.say(format!(..))` → `courier::reply(ctx, CrackedMessage::Other(..))`, keeping today's text.
- `:260` and `:264` `send_reply_embed` → `courier::reply`.
- `:557` and `:638` `ctx.send_message(params)` → `courier::reply_rendered` / `courier::reply_as`.

**Other files:**
- `music/query.rs:352`: the channel `send_message` becomes `courier::post(.., Destination::Channel(..), ..)` with its content as a `CrackedMessage`. `:610` `edit_response_poise` keeps its wrapper, which is courier-backed since Task 5.
- `music/queue.rs:527,619`: the playlist progress `msg.edit(EditMessage::new().embed(..description(text)))` becomes `courier::edit_message(&transport, msg.channel_id, msg.id, &CrackedMessage::Other(text))`. Add to `courier.rs` a `pub async fn edit_message(transport: &dyn Transport, channel, id, msg: &CrackedMessage) -> Result<(), TransportError>` that renders with `RenderCx::now()`, plus a test against `FakeTransport` pinning `Op::Edit(channel, id)`. Build the transport from `ctx.serenity_context()` (`DiscordTransport { http: ctx.serenity_context().http.clone(), cache: ctx.serenity_context().cache.clone() }`).
- `dosearch.rs:65` and `interface.rs::create_search_results_reply`: build a `Rendered` (embeds as `CreateEmbed` variants; if more than one embed is sent today, add `pub embeds_extra: Vec<CreateEmbed<'static>>` to `Rendered`, and include it in `to_reply`, `to_message` and `to_edit`) and send it with `courier::reply_rendered`.
- `utils.rs::yt_search_select`:
  - the select-menu `send_message` becomes `courier::post(.., Destination::Channel)`, with components on the `Rendered`. It needs the sent message id for the collector, so add `courier::post_message(transport, channel, &Rendered) -> Result<MessageId, TransportError>`;
  - the `create_response(UpdateMessage)` stays and is marked `#[expect(clippy::disallowed_methods, reason = "interaction responses move in PR 2")]` in Task 10;
  - `m.reply(.., "Timed out")` becomes `courier::post(.., Destination::Channel(channel_id), &CrackedMessage::Other(..))`.

**Tests:**
- `cards::queued`:
  - unknown duration and wait → no footer at all;
  - known both → `"Track duration: 4:33\nEstimated time until play: 0:48"` (pin the real `TRACK_DURATION` and `TRACK_TIME_TO_PLAY` text as literals);
  - untitled → `(untitled)`.
- `build_play_reply`'s content path: without `EMBED_LINKS`, the notice is in `content` and the embed is kept.
- `courier::edit_message` → `Op::Edit`.

Sabotage each, then commit with message "/play: queued card without 00:00, and every play reply through the courier".

---

### Task 8: Paging (queue, nowplaying, lyrics, collectors)

**Files:** `commands/music/queue.rs`, `commands/music/nowplaying.rs`, `commands/music/collector.rs`, `commands/music/lyrics.rs` (via `utils::create_paged_embed`), `messaging/interface.rs` (`create_queue_page`, `create_queue_embed`, `create_nav_btns`), `handlers/track_end.rs::update_queue_messages`, `utils.rs::create_paged_embed`, `utils.rs::build_queue_page_metadata`.

**Rules:**
1. **Queue page entries:**
   - each line's title is `TrackLabel::from_metadata(&meta).linked(INLINE_TITLE_MAX)`;
   - each duration is `duration_text(meta.duration)`, and when that is `None` the duration part is omitted (no `00:00`);
   - this replaces `QUEUE_NO_TITLE` and `QUEUE_NO_SRC` in these builders. Leave those constants if anything else uses them.
2. **Paging sends and edits:**
   - the first send goes through `courier::reply_rendered(ctx, Rendered::embed(embed).with_components(btns), false)`;
   - each page flip's `mci.create_response(ctx, UpdateMessage(...))` stays, because interaction responses are PR 2. Mark it `#[expect(clippy::disallowed_methods, reason = "component responses move in PR 2")]` at the statement in Task 10, and build its message from a `Rendered` (`CreateInteractionResponseMessage::new().embed(..).components(..)`) so the formatting rules still apply;
   - the expiry edit `EditMessage::new().embed(..QUEUE_EXPIRED)` becomes `courier::edit_message`.
3. **`update_queue_messages`:** `message.edit(cache_http, EditMessage::new().embed(embed).components(..))` becomes `courier::edit_message` with `CrackedMessage::CreateEmbed` plus components. Add `courier::edit_rendered_message(transport, channel, id, Rendered)` if components are needed.
4. **`nowplaying.rs`:** `pointer_reply(..)`'s `CreateReply` becomes `courier::reply_as(ctx, CrackedMessage::Other(pointer), private)`. Escape the title passed to `now_playing_pointer`: today `/nowplaying` interpolates the raw title. Build it with `TrackLabel::title_text(INLINE_TITLE_MAX)`, and add a test pinning that `now_playing_pointer` receives an escaped title, e.g. `"a\\*b"` for a track titled `a*b`.
5. **`collector.rs`:** its `ctx.send(CreateReply..)` becomes `courier::reply_rendered`; its `mci.create_response` is handled like rule 2.

**Tests:** a queue page with an untitled, URL-less, duration-less track renders `**(untitled)**` with no `00:00` (pinned); and the `/nowplaying` pointer escaping test. Sabotage each, then commit with message "Queue and lyrics paging through the courier; queue lines without 00:00".

---

### Task 9: The remaining music commands

**Sites:**
- `auditlog.rs:43,66,102`: `ctx.send(CreateReply::default().content(text).ephemeral(e))` becomes `courier::reply_rendered(ctx, Rendered::text(text), e)`.
- `playlog.rs:54`, `autoplay.rs:42`: `send_reply` → `courier::reply`.
- `autopause.rs:46`, `ephemeral.rs:44`: `ctx.send_message(params)` → `courier::reply(ctx, params.msg)`.
- `vote.rs:45`: `ctx.reply(format!(..))` → `courier::reply_rendered(ctx, Rendered::text(..), false)`, same text.
- `voteskip.rs:43`, `grab.rs:35`, `summon.rs:98`, `music_utils.rs:273`: `send_reply_embed` → `courier::reply`.
- `diagnose.rs:145`: `ctx.say(out)` → `courier::reply_rendered(ctx, Rendered::text(out), false)`.
- `spotify.rs:202`: `ctx.send(CreateReply::default().embed(embed))` → `courier::reply(ctx, CrackedMessage::CreateEmbed(Box::new(embed)))`.
- `gambling.rs:11,40`: `ctx.send_reply(msg, true)` → `courier::reply`.
- `get_metadata.rs:75`: `edit_embed_response2` is already courier-backed; leave it.
- `music_utils.rs:286-292`: the plain `CreateReply` → `courier::reply_rendered(ctx, Rendered::text(..), false)`, keeping the 🪤 comment's reasoning (why text, not embed).

**Tests:** for each command whose text contains third-party text, add a pinned test that the text is escaped. If none does, say so in the report. Commit with message "Remaining music commands reply through the courier".

---

### Task 10: The ban

**Files:** `clippy.toml`; the module headers of `commands/music/gp.rs`, `commands/music/gp_persist.rs`, `commands/admin/**`, `commands/osint/**` (if present), `commands/settings/**`, `commands/register.rs`, `commands/utility/**`, `commands/help.rs`, `commands/premium.rs`, `handlers/voice_chat_stats.rs`, `handlers/serenity.rs` (the welcome and log-system-load statements only, at statement level), `messaging/**` (the module that sends), `http_utils.rs` if it sends, and anything else clippy names.

- [ ] **Step 1: Find the exact paths clippy resolves.** Add a scratch `disallowed-methods` entry for each candidate below, run `cargo clippy -p crack-core --all-targets 2>&1 | grep disallowed`, and keep only the entries that clippy reports at a real call site. A path clippy cannot resolve is silently ignored, so an unverified entry is no ban at all. Candidates:

```toml
{ path = "serenity::model::id::GenericChannelId::send_message", reason = "send through messaging::courier" },
{ path = "serenity::model::id::GenericChannelId::say", reason = "send through messaging::courier" },
{ path = "serenity::model::id::GenericChannelId::edit_message", reason = "edit through messaging::courier" },
{ path = "serenity::model::id::ChannelId::send_message", reason = "send through messaging::courier" },
{ path = "serenity::model::id::ChannelId::say", reason = "send through messaging::courier" },
{ path = "serenity::model::channel::Message::reply", reason = "send through messaging::courier" },
{ path = "serenity::model::channel::Message::edit", reason = "edit through messaging::courier" },
{ path = "poise::structs::context::Context::send", reason = "reply through messaging::courier" },
{ path = "poise::structs::context::Context::say", reason = "reply through messaging::courier" },
{ path = "poise::structs::context::Context::reply", reason = "reply through messaging::courier" },
{ path = "poise::reply::ReplyHandle::edit", reason = "edit through messaging::courier" },
{ path = "poise::reply::send_reply", reason = "reply through messaging::courier" },
```

If a method is defined on a trait or in a differently named module, find its real path in the pinned checkouts (`~/.cargo/git/checkouts/serenity-*/37b9f43`, `poise-*/b189f7c`). Record the final list in the report.
- [ ] **Step 2: Add the verified entries** to `clippy.toml` under the existing `disallowed-methods` list, beside the `music::queue` bans.
- [ ] **Step 3: Allow the module that sends.** In `messaging/courier.rs`, `messaging/transport.rs` and `poise_ext.rs`/`utils.rs`, if their wrappers still exist, put `#[expect(clippy::disallowed_methods, reason = "messaging is where sends are made")]` on each sending statement or function.
- [ ] **Step 4: Mark every unmigrated module.** Add the module-level `#![expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]` to each one clippy names. **No module under `commands/music/` (except `gp.rs`, `gp_persist.rs`), `music/`, `handlers/track_end.rs` or `handlers/idle.rs` may get one.** If clippy flags one of those, migrate the site instead.
- [ ] **Step 5: Delete dead wrappers.** Delete each wrapper in `utils.rs` and `poise_ext.rs` that nothing calls any more (`cargo clippy` with `dead_code` will tell you for private ones; for `pub` ones use `rg`). Remove the `colored` import from `poise_ext.rs`. Keep `colored` in `Cargo.toml` if logs still use it.
- [ ] **Step 6: Show the ban bites.** Add `let _ = serenity::all::GenericChannelId::new(1).say(&ctx, "x").await;` to `commands/music/skip.rs`, run clippy, see `disallowed_methods`, then remove it. Write the clippy line into the report.
- [ ] **Step 7: Final checks and commit.** Run `cargo +nightly fmt --all`, `cargo clippy --workspace --all-targets` (clean), and `cargo test --workspace` (PASS). Commit with message "clippy: no raw sends outside messaging; the rest are marked as not migrated".

---

### Task 11: Release prep

**Files:** `Cargo.toml` (workspace `version = "0.21.1"` → `"0.22.0"`), `Cargo.lock` (via `cargo update -w`), `CHANGELOG.md` (the `## Unreleased` section's `### Changed` / `### Fixed`), and `docs/` (if a doc describes message sending, such as `docs/web-dashboard.md`'s echo line; update only what changed).

**CHANGELOG entries.** Third person, past and present tense as the file already uses:
- **Fixed:**
  - a track without a title read "Skipped to **!" and similar → now "(untitled)";
  - titles with `*`, `_`, `@` or brackets broke the message's formatting or could ping;
  - "Track duration: 00:00" and the estimate built on it are gone when the length is unknown;
  - no more `RelativeUrlWithoutBase` log or "Streaming via unknown" for tracks without a link or thumbnail;
  - `/nowplaying` escaped the title.
- **Changed:**
  - the now-playing message shows when the track ends, counted down live by Discord, or "Paused at m:ss";
  - durations read `m:ss`, or `h:mm:ss` from an hour up;
  - the autoplay notices are embeds like every other notice;
  - every music message goes through one renderer.

- [ ] **Step 1:** Bump the version, run `cargo update -w`, write the CHANGELOG, then fmt, clippy and test.
- [ ] **Step 2:** Commit with message "v0.22.0: one messaging layer for every music message".
