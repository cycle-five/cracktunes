//! The structured messages: now playing, queued, the echo of a dashboard
//! control. This task holds the types; their renderers arrive with the
//! messages that use them.
use crate::messaging::format::{cap, escape, Progress, TrackLabel, INLINE_TITLE_MAX};
use crate::messaging::messages::{
    ECHO_FROM_DASHBOARD, ECHO_PAUSED, ECHO_REMOVED, ECHO_REPEAT_OFF, ECHO_REPEAT_ON, ECHO_RESUMED,
    ECHO_SHUFFLED, ECHO_SKIPPED,
};
use crate::messaging::render::{RenderCx, Rendered};
use serenity::all::{CreateEmbed, UserId};
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

/// Stub until the now-playing card moves here.
#[must_use]
pub fn now_playing(_card: &NowPlayingCard, _cx: &RenderCx) -> Rendered {
    Rendered::default()
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
