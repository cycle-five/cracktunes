//! The structured messages: now playing, queued, the echo of a dashboard or
//! button control, and their renderers.
use crate::format::{
    clip, duration_text, http_url, progress_text, Progress, TrackLabel, AUTHOR_MAX,
    EMBED_TITLE_MAX, FIELD_MAX, INLINE_TITLE_MAX,
};
use crate::messages::{
    ECHO_ADDED, ECHO_ADDED_NEXT, ECHO_FROM_DASHBOARD, ECHO_PAUSED, ECHO_REMOVED, ECHO_REPEAT_OFF,
    ECHO_REPEAT_ON, ECHO_RESUMED, ECHO_SHUFFLED, ECHO_SKIPPED, INVITE_LINK_TEXT_SHORT, INVITE_URL,
    PROGRESS, QUEUE_NOW_PLAYING, REQUESTED_BY, STATUS_FINISHED_DESCRIPTION, STATUS_FINISHED_TITLE,
    TRACK_DURATION, TRACK_TIME_TO_PLAY, VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL,
};
use crate::render::{RenderCx, Rendered};
use crack_model::{Author, Card, Field, Footer, Mention, Row, UserRef};
use std::time::Duration;

/// The now-playing card.
#[derive(Debug, Clone)]
pub struct NowPlayingCard {
    pub label: TrackLabel,
    pub thumbnail: Option<String>,
    pub requester: Option<UserRef>,
    pub progress: Progress,
    /// The buttons: only the status message has rows, never a reply or a DM.
    pub rows: Vec<Row>,
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
    /// A now-playing button, pressed in Discord.
    Button,
}

/// A control's echo line, with who did it and from where.
#[derive(Debug, Clone)]
pub struct EchoLine {
    pub echo: Echo,
    pub user: UserRef,
    pub via: Via,
}

impl EchoLine {
    #[must_use]
    pub fn line(&self) -> String {
        self.echo.line(&self.user, self.via)
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
    Added { title: Option<String> },
    AddedNext { title: Option<String> },
}

impl Echo {
    /// The one line posted in Discord. Titles are third-party text, so they
    /// are cut to `INLINE_TITLE_MAX` characters, then escaped; a blank one is
    /// `(untitled)`, never `****`.
    #[must_use]
    pub fn line(&self, user: &UserRef, via: Via) -> String {
        let (what, title) = match self {
            Self::Skipped { title } => (ECHO_SKIPPED, title.as_deref()),
            Self::Paused => (ECHO_PAUSED, None),
            Self::Resumed => (ECHO_RESUMED, None),
            Self::Repeat { on: true } => (ECHO_REPEAT_ON, None),
            Self::Repeat { on: false } => (ECHO_REPEAT_OFF, None),
            Self::Removed { title } => (ECHO_REMOVED, title.as_deref()),
            Self::Shuffled => (ECHO_SHUFFLED, None),
            Self::Added { title } => (ECHO_ADDED, title.as_deref()),
            Self::AddedNext { title } => (ECHO_ADDED_NEXT, title.as_deref()),
        };
        // A press in Discord is visibly in Discord; the dashboard says so.
        let from = match via {
            Via::Dashboard => format!(" {ECHO_FROM_DASHBOARD}"),
            Via::Button => String::new(),
        };
        match title {
            Some(t) => format!(
                "{what} **{}**{from} — <@{}>",
                TrackLabel {
                    title: Some(t.to_owned()),
                    ..TrackLabel::default()
                }
                .title_text(INLINE_TITLE_MAX),
                user.raw().as_str()
            ),
            None => format!("{what}{from} — <@{}>", user.raw().as_str()),
        }
    }
}

/// The now-playing embed. Everything that comes from a track is optional and
/// third-party: a missing or relative URL leaves out the link, the thumbnail
/// and the footer instead of rendering a broken one.
#[must_use]
pub fn now_playing(card: &NowPlayingCard, cx: &RenderCx) -> Rendered {
    let url = http_url(card.label.url.as_deref());
    let requester = match &card.requester {
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
    let mut out = Card {
        title: Some(title),
        description: Some(vanity),
        author: Some(Author {
            name: clip(QUEUE_NOW_PLAYING, AUTHOR_MAX),
            url: None,
            icon_url: None,
        }),
        fields: vec![
            Field {
                name: PROGRESS.to_owned(),
                value: progress,
                inline: true,
            },
            Field {
                name: REQUESTED_BY.to_owned(),
                value: requester,
                inline: true,
            },
        ],
        ..Card::default()
    };
    if let Some(u) = &url {
        out.url = Some(u.to_string());
        if u.host_str().is_some() {
            out.footer = Some(Footer {
                text: footer_text,
                icon_url: Some(footer_icon),
            });
        }
    }
    out.thumbnail = http_url(card.thumbnail.as_deref()).map(|t| t.to_string());
    let rendered = Rendered::card(out);
    if card.rows.is_empty() {
        rendered
    } else {
        rendered.with_rows(card.rows.clone())
    }
}

/// Converts a user id to a string, with special handling for autoplay.
pub fn requesting_user_to_string(user: &UserRef) -> String {
    match user.raw().as_str() {
        "1" => "(auto)".to_string(),
        _ => Mention::User(user.clone()).to_string(),
    }
}

/// Build the strings used for the footer of an embed from a given url.
pub fn build_footer_info(url: &str) -> (String, String, String) {
    let vanity = format!(
        "[{}]({}) • [{}]({})",
        VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL, INVITE_LINK_TEXT_SHORT, INVITE_URL,
    );
    let url_data = match url::Url::parse(url) {
        Ok(url_data) => url_data,
        Err(_) => {
            return (
                "Streaming via unknown".to_string(),
                "https://www.google.com/s2/favicons?domain=unknown".to_string(),
                vanity,
            )
        },
    };
    let domain = url_data.host_str().unwrap();

    // remove www prefix because it looks ugly
    let domain = domain.replace("www.", "");

    (
        format!("Streaming via {}", domain),
        format!("https://www.google.com/s2/favicons?domain={}", domain),
        vanity,
    )
}

/// The "Finished" status.
pub fn finished_card() -> crack_model::Card {
    crack_model::Card {
        title: Some(STATUS_FINISHED_TITLE.to_owned()),
        description: Some(STATUS_FINISHED_DESCRIPTION.to_owned()),
        ..crack_model::Card::default()
    }
}

#[must_use]
pub fn finished() -> Rendered {
    Rendered::card(finished_card())
}

/// The card for a track added to the queue. Its length and its wait are each
/// left out when unknown, never shown as `00:00`: an unknown wait is a live
/// stream (or a track of unknown length) ahead of it.
#[must_use]
pub fn queued(card: &QueuedCard, _cx: &RenderCx) -> Rendered {
    // `title_text` escapes after capping, so the escaped title can be longer.
    let title = clip(&card.label.title_text(EMBED_TITLE_MAX), EMBED_TITLE_MAX);
    let footer: Vec<String> = [
        duration_text(card.label.duration).map(|d| format!("{TRACK_DURATION} {d}")),
        duration_text(card.wait).map(|w| format!("{TRACK_TIME_TO_PLAY} {w}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let out = Card {
        title: Some(title),
        url: http_url(card.label.url.as_deref()).map(|u| u.to_string()),
        author: Some(Author {
            name: clip(card.author, AUTHOR_MAX),
            url: None,
            icon_url: None,
        }),
        thumbnail: http_url(card.thumbnail.as_deref()).map(|t| t.to_string()),
        footer: (!footer.is_empty()).then(|| Footer {
            text: footer.join("\n"),
            icon_url: None,
        }),
        ..Card::default()
    };
    Rendered::card(out)
}

#[must_use]
pub fn echo(line: &EchoLine) -> Rendered {
    Rendered::card(Card {
        description: Some(line.line()),
        ..Card::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Progress, TrackLabel};
    use crack_types::discord::ToRef;
    use serenity::all::UserId;
    use std::time::Duration;
    fn card(url: Option<&str>, thumb: Option<&str>) -> NowPlayingCard {
        NowPlayingCard {
            label: TrackLabel {
                title: Some("OH SHIT I'M FEELING IT".into()),
                url: url.map(str::to_owned),
                duration: Some(Duration::from_secs(273)),
            },
            thumbnail: thumb.map(str::to_owned),
            requester: Some(UserId::new(42).to_ref()),
            progress: Progress::Playing {
                position: Duration::from_secs(73),
                duration: Some(Duration::from_secs(273)),
            },
            rows: Vec::new(),
        }
    }

    /// Plan 2026-10-07-now-playing-buttons, Ruling 4: a press in Discord needs no "from ..."; the dashboard keeps its.
    #[test]
    fn a_button_echo_has_no_source_suffix() {
        let skipped = Echo::Skipped {
            title: Some("t0".into()),
        };
        assert_eq!(
            skipped.line(&UserId::new(42).to_ref(), Via::Button),
            "⏭ Skipped **t0** — <@42>"
        );
        assert_eq!(
            skipped.line(&UserId::new(42).to_ref(), Via::Dashboard),
            "⏭ Skipped **t0** from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Paused.line(&UserId::new(42).to_ref(), Via::Button),
            "⏸ Paused — <@42>"
        );
        assert_eq!(
            Echo::Repeat { on: true }.line(&UserId::new(42).to_ref(), Via::Button),
            "🔁 Repeat on — <@42>"
        );
    }

    #[test]
    fn add_echoes_name_the_track_where_it_went_and_who() {
        let u = UserId::new(5).to_ref();
        assert_eq!(
            Echo::Added {
                title: Some("Waterloo".into())
            }
            .line(&u, Via::Dashboard),
            format!("{ECHO_ADDED} **Waterloo** {ECHO_FROM_DASHBOARD} — <@5>")
        );
        assert_eq!(
            Echo::AddedNext {
                title: Some("Waterloo".into())
            }
            .line(&u, Via::Dashboard),
            format!("{ECHO_ADDED_NEXT} **Waterloo** {ECHO_FROM_DASHBOARD} — <@5>")
        );
        let hostile = Echo::Added {
            title: Some("**x** @everyone".into()),
        }
        .line(&u, Via::Dashboard);
        assert!(
            !hostile.contains("**x**"),
            "the title is escaped: {hostile}"
        );
        for blank in ["", "   "] {
            let line = Echo::AddedNext {
                title: Some(blank.into()),
            }
            .line(&u, Via::Dashboard);
            assert!(line.contains("**(untitled)**"), "{blank:?}: {line}");
        }
    }

    /// A blank title (a members-only link queues `Some("")`) echoed as
    /// "⏭ Skipped **** …"; it is `(untitled)`, as everywhere else.
    #[test]
    fn an_echo_of_a_blank_title_says_untitled() {
        for blank in ["", "   "] {
            let echo = Echo::Skipped {
                title: Some(blank.into()),
            };
            assert_eq!(
                echo.line(&UserId::new(42).to_ref(), Via::Dashboard),
                "⏭ Skipped **(untitled)** from the dashboard — <@42>",
                "{blank:?}"
            );
        }
    }

    /// Autoplay is stored as user 1; the card says so instead of mentioning it.
    #[test]
    fn an_autoplayed_card_names_the_requester_auto() {
        let mut c = card(None, None);
        c.requester = Some(UserId::new(1).to_ref());
        let out = now_playing(&c, &RenderCx::default());
        assert_eq!(out.cards[0].fields[1].value, ">>> (auto)");
    }

    const VANITY: &str = "[vote](https://top.gg/bot/1115229568006103122/vote) • [invite](https://discord.com/oauth2/authorize?client_id=1115229568006103122&permissions=551940115520&scope=bot+applications.commands)";

    #[test]
    fn the_footer_names_the_host_without_www() {
        assert_eq!(
            build_footer_info("https://www.youtube.com/watch?v=x"),
            (
                "Streaming via youtube.com".to_owned(),
                "https://www.google.com/s2/favicons?domain=youtube.com".to_owned(),
                VANITY.to_owned()
            )
        );
    }

    #[test]
    fn an_unparseable_url_streams_via_unknown() {
        assert_eq!(
            build_footer_info("not a url"),
            (
                "Streaming via unknown".to_owned(),
                "https://www.google.com/s2/favicons?domain=unknown".to_owned(),
                VANITY.to_owned()
            )
        );
    }

    #[test]
    fn the_finished_card_has_its_title_and_description() {
        assert_eq!(
            finished_card(),
            crack_model::Card {
                title: Some("⏹️ Finished".to_owned()),
                description: Some("Nothing is playing. Use /play to start again.".to_owned()),
                ..crack_model::Card::default()
            }
        );
        assert_eq!(finished().cards, vec![finished_card()]);
    }

    fn queued_card(url: Option<&str>, secs: Option<u64>, wait: Option<u64>) -> QueuedCard {
        QueuedCard {
            author: "Added to queue",
            label: TrackLabel {
                title: Some("Waterloo".into()),
                url: url.map(str::to_owned),
                duration: secs.map(Duration::from_secs),
            },
            thumbnail: url.map(|_| "https://i.ytimg.com/t.jpg".to_owned()),
            wait: wait.map(Duration::from_secs),
        }
    }

    #[test]
    fn a_queued_track_shows_link_thumbnail_length_and_wait() {
        let out = queued(
            &queued_card(Some("https://youtu.be/x"), Some(273), Some(60)),
            &RenderCx::default(),
        );
        assert_eq!(
            out.cards,
            vec![Card {
                title: Some("Waterloo".to_owned()),
                url: Some("https://youtu.be/x".to_owned()),
                author: Some(Author {
                    name: "Added to queue".to_owned(),
                    url: None,
                    icon_url: None,
                }),
                thumbnail: Some("https://i.ytimg.com/t.jpg".to_owned()),
                footer: Some(Footer {
                    text: "Track duration: 4:33\nEstimated time until play: 1:00".to_owned(),
                    icon_url: None,
                }),
                ..Card::default()
            }]
        );
    }

    #[test]
    fn a_queued_track_without_link_length_or_wait_has_no_footer() {
        let out = queued(&queued_card(None, None, None), &RenderCx::default());
        assert_eq!(
            out.cards,
            vec![Card {
                title: Some("Waterloo".to_owned()),
                author: Some(Author {
                    name: "Added to queue".to_owned(),
                    url: None,
                    icon_url: None,
                }),
                ..Card::default()
            }]
        );
    }
}
