//! The structured messages: now playing, queued, the echo of a dashboard
//! control. This task holds the types; their renderers arrive with the
//! messages that use them.
use crate::messaging::format::{
    cap, clip, escape, http_url, progress_text, Progress, TrackLabel, AUTHOR_MAX, EMBED_TITLE_MAX,
    FIELD_MAX, INLINE_TITLE_MAX,
};
use crate::messaging::interface::requesting_user_to_string;
use crate::messaging::messages::{
    ECHO_FROM_DASHBOARD, ECHO_PAUSED, ECHO_REMOVED, ECHO_REPEAT_OFF, ECHO_REPEAT_ON, ECHO_RESUMED,
    ECHO_SHUFFLED, ECHO_SKIPPED, PROGRESS, QUEUE_NOW_PLAYING, REQUESTED_BY,
};
use crate::messaging::render::{RenderCx, Rendered};
use crate::utils::build_footer_info;
use serenity::all::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, UserId};
use std::time::Duration;

/// The now-playing card.
#[derive(Debug, Clone)]
pub struct NowPlayingCard {
    pub label: TrackLabel,
    pub thumbnail: Option<String>,
    pub requester: Option<UserId>,
    pub progress: Progress,
}

/// A track or list added to the queue.
#[derive(Debug, Clone)]
pub struct QueuedCard {
    pub author: &'static str,
    pub label: TrackLabel,
    pub thumbnail: Option<String>,
    pub wait: Option<Duration>,
}

/// Where a control came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Dashboard,
}

/// A control's echo line, with who did it and from where.
#[derive(Debug, Clone)]
pub struct EchoLine {
    pub echo: Echo,
    pub user: UserId,
    pub via: Via,
}

impl EchoLine {
    #[must_use]
    pub fn line(&self) -> String {
        self.echo.line(self.user)
    }
}

/// What a control did, for the echo line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Echo {
    Skipped { title: Option<String> },
    Paused,
    Resumed,
    Repeat { on: bool },
    Removed { title: Option<String> },
    Shuffled,
}

impl Echo {
    /// The one line posted in Discord. Titles are third-party text, so they
    /// are cut to `INLINE_TITLE_MAX` characters, then escaped.
    #[must_use]
    pub fn line(&self, user: UserId) -> String {
        let (what, title) = match self {
            Self::Skipped { title } => (ECHO_SKIPPED, title.as_deref()),
            Self::Paused => (ECHO_PAUSED, None),
            Self::Resumed => (ECHO_RESUMED, None),
            Self::Repeat { on: true } => (ECHO_REPEAT_ON, None),
            Self::Repeat { on: false } => (ECHO_REPEAT_OFF, None),
            Self::Removed { title } => (ECHO_REMOVED, title.as_deref()),
            Self::Shuffled => (ECHO_SHUFFLED, None),
        };
        match title {
            Some(t) => format!(
                "{what} **{}** {ECHO_FROM_DASHBOARD} — <@{user}>",
                escape(&cap(t, INLINE_TITLE_MAX))
            ),
            None => format!("{what} {ECHO_FROM_DASHBOARD} — <@{user}>"),
        }
    }
}

/// The now-playing embed. Everything that comes from a track is optional and
/// third-party: a missing or relative URL leaves out the link, the thumbnail
/// and the footer instead of rendering a broken one.
#[must_use]
pub fn now_playing(card: &NowPlayingCard, cx: &RenderCx) -> Rendered {
    let url = http_url(card.label.url.as_deref());
    let requester = match card.requester {
        Some(id) => format!(">>> {}", requesting_user_to_string(id)),
        None => ">>> N/A".to_owned(),
    };
    let progress = clip(
        &format!(">>> {}", progress_text(&card.progress, cx.now_unix)),
        FIELD_MAX,
    );
    // `title_text` escapes after capping, so the escaped title can be longer.
    let title = clip(&card.label.title_text(EMBED_TITLE_MAX), EMBED_TITLE_MAX);
    let (footer_text, footer_icon, vanity) =
        build_footer_info(url.as_ref().map_or("", |u| u.as_str()));
    let mut embed = CreateEmbed::new()
        .author(CreateEmbedAuthor::new(clip(QUEUE_NOW_PLAYING, AUTHOR_MAX)))
        .title(title)
        .field(PROGRESS, progress, true)
        .field(REQUESTED_BY, requester, true)
        .description(vanity);
    if let Some(u) = &url {
        embed = embed.url(u.to_string());
        if u.host_str().is_some() {
            embed = embed.footer(CreateEmbedFooter::new(footer_text).icon_url(footer_icon));
        }
    }
    if let Some(t) = http_url(card.thumbnail.as_deref()) {
        embed = embed.thumbnail(t.to_string(), None);
    }
    Rendered::embed(embed)
}

#[must_use]
pub fn finished() -> Rendered {
    Rendered::embed(crate::messaging::status::finished_embed())
}

/// Stub until the queued card moves here.
#[must_use]
pub fn queued(_card: &QueuedCard, _cx: &RenderCx) -> Rendered {
    Rendered::default()
}

#[must_use]
pub fn echo(line: &EchoLine) -> Rendered {
    Rendered::embed(CreateEmbed::new().description(line.line()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::{Progress, TrackLabel};
    use crate::messaging::messages::STATUS_FINISHED_TITLE;
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
            progress: Progress::Playing {
                position: Duration::from_secs(73),
                duration: Some(Duration::from_secs(273)),
            },
        }
    }

    #[test]
    fn the_now_playing_card_shows_a_live_end_time() {
        let r = now_playing(
            &card(
                Some("https://www.youtube.com/watch?v=x"),
                Some("https://i.ytimg.com/a.jpg"),
            ),
            &RenderCx {
                now_unix: 1_000_000,
                embed_links: true,
            },
        );
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
        c.progress = Progress::Playing {
            position: Duration::ZERO,
            duration: None,
        };
        let e = v(&now_playing(
            &c,
            &RenderCx {
                now_unix: 50,
                embed_links: true,
            },
        ));
        assert_eq!(e["title"], "(untitled)");
        assert!(e.get("url").is_none() || e["url"].is_null());
        assert!(e.get("thumbnail").is_none() || e["thumbnail"].is_null());
        assert!(e.get("footer").is_none() || e["footer"].is_null());
        assert_eq!(e["fields"][0]["value"], ">>> Started <t:50:R>");
    }

    /// A link that is not http(s) is not a link: no url, thumbnail or footer.
    #[test]
    fn a_non_http_url_is_not_rendered() {
        let e = v(&now_playing(
            &card(Some("javascript:alert(1)"), Some("file:///etc/passwd")),
            &RenderCx::default(),
        ));
        for key in ["url", "thumbnail", "footer"] {
            assert!(e.get(key).is_none() || e[key].is_null(), "{key}: {e}");
        }
    }

    #[test]
    fn a_paused_card_says_where_it_paused() {
        let mut c = card(None, None);
        c.progress = Progress::Paused {
            position: Some(Duration::from_secs(72)),
        };
        assert_eq!(
            v(&now_playing(
                &c,
                &RenderCx {
                    now_unix: 0,
                    embed_links: true
                }
            ))["fields"][0]["value"],
            ">>> Paused at 1:12"
        );
    }

    /// Ruling R2: escaping after the cap can push a title past Discord's limit.
    #[test]
    fn an_escaped_title_still_fits_the_embed_title() {
        let mut c = card(None, None);
        c.label.title = Some("*".repeat(300));
        let e = v(&now_playing(&c, &RenderCx::default()));
        assert!(e["title"].as_str().unwrap().chars().count() <= EMBED_TITLE_MAX);
    }

    #[test]
    fn the_finished_card_is_the_status_embed() {
        let e = v(&finished());
        assert_eq!(e["title"], STATUS_FINISHED_TITLE);
    }
}
