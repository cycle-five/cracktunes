//! The structured messages: now playing, queued, the echo of a dashboard or
//! button control, and their renderers.
use crate::messaging::format::{
    clip, duration_text, http_url, progress_text, Progress, TrackLabel, AUTHOR_MAX,
    EMBED_TITLE_MAX, FIELD_MAX, INLINE_TITLE_MAX,
};
use crate::messaging::messages::{
    ECHO_ADDED, ECHO_ADDED_NEXT, ECHO_FROM_DASHBOARD, ECHO_PAUSED, ECHO_REMOVED, ECHO_REPEAT_OFF,
    ECHO_REPEAT_ON, ECHO_RESUMED, ECHO_SHUFFLED, ECHO_SKIPPED, INVITE_LINK_TEXT_SHORT, INVITE_URL,
    PROGRESS, QUEUE_NOW_PLAYING, REQUESTED_BY, STATUS_FINISHED_DESCRIPTION, STATUS_FINISHED_TITLE,
    TRACK_DURATION, TRACK_TIME_TO_PLAY, VOTE_TOPGG_LINK_TEXT_SHORT, VOTE_TOPGG_URL,
};
use crate::messaging::render::{RenderCx, Rendered};
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
    use crate::messaging::discord::ToDiscord;
    use crate::messaging::format::{Progress, TrackLabel};
    use crate::messaging::messages::STATUS_FINISHED_TITLE;
    use crate::messaging::render::RenderCx;
    use crack_types::discord::ToRef;
    use serenity::all::UserId;
    use std::time::Duration;

    fn v(r: &Rendered) -> serde_json::Value {
        serde_json::to_value(crate::messaging::discord::first_embed(r).unwrap()).unwrap()
    }

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

    /// Ruling R9: a track on repeat never reaches "ends …", so the card says
    /// it is on repeat instead of an end time that goes stale.
    #[test]
    fn a_card_on_repeat_has_no_end_time() {
        let mut c = card(None, None);
        c.progress = Progress::Repeating {
            duration: Some(Duration::from_secs(273)),
        };
        let cx = RenderCx {
            now_unix: 1_000_000,
            embed_links: true,
        };
        assert_eq!(
            v(&now_playing(&c, &cx))["fields"][0]["value"],
            ">>> 4:33 · on repeat"
        );
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

    fn queued_card(duration: Option<Duration>, wait: Option<Duration>) -> QueuedCard {
        QueuedCard {
            author: "📃 Added to queue!",
            label: TrackLabel {
                title: Some("OH SHIT I'M FEELING IT".into()),
                url: Some("https://www.youtube.com/watch?v=x".into()),
                duration,
            },
            thumbnail: Some("https://i.ytimg.com/a.jpg".into()),
            wait,
        }
    }

    /// "Added to queue! Track duration: 00:00" was computed from a length
    /// nobody knew; now an unknown length and wait say nothing at all.
    #[test]
    fn a_queued_track_of_unknown_length_has_no_footer() {
        let e = v(&queued(&queued_card(None, None), &RenderCx::default()));
        assert!(e.get("footer").is_none() || e["footer"].is_null(), "{e}");
        assert_eq!(e["author"]["name"], "📃 Added to queue!");
        assert_eq!(e["title"], "OH SHIT I'M FEELING IT");
        assert_eq!(e["url"], "https://www.youtube.com/watch?v=x");
        assert_eq!(e["thumbnail"]["url"], "https://i.ytimg.com/a.jpg");
    }

    #[test]
    fn a_queued_track_says_its_length_and_its_wait() {
        let e = v(&queued(
            &queued_card(
                Some(Duration::from_secs(273)),
                Some(Duration::from_secs(48)),
            ),
            &RenderCx::default(),
        ));
        assert_eq!(
            e["footer"]["text"],
            "Track duration: 4:33\nEstimated time until play: 0:48"
        );
    }

    /// Each line stands alone: a known length with an unknown wait (a live
    /// stream ahead of it) still says the length.
    #[test]
    fn a_queued_track_with_an_unknown_wait_says_only_its_length() {
        let e = v(&queued(
            &queued_card(Some(Duration::from_secs(273)), None),
            &RenderCx::default(),
        ));
        assert_eq!(e["footer"]["text"], "Track duration: 4:33");
    }

    #[test]
    fn an_untitled_queued_track_is_named_untitled() {
        let mut c = queued_card(None, None);
        c.label.title = Some("  ".into());
        assert_eq!(v(&queued(&c, &RenderCx::default()))["title"], "(untitled)");
    }

    /// A link or thumbnail that is not http(s) is left out, not sent broken.
    #[test]
    fn a_queued_track_without_a_web_link_has_no_url_or_thumbnail() {
        let mut c = queued_card(None, None);
        c.label.url = Some("javascript:alert(1)".into());
        c.thumbnail = Some(String::new());
        let e = v(&queued(&c, &RenderCx::default()));
        for key in ["url", "thumbnail"] {
            assert!(e.get(key).is_none() || e[key].is_null(), "{key}: {e}");
        }
    }

    /// Ruling R2, for the queued card too.
    #[test]
    fn an_escaped_queued_title_still_fits_the_embed_title() {
        let mut c = queued_card(None, None);
        c.label.title = Some("*".repeat(300));
        let e = v(&queued(&c, &RenderCx::default()));
        assert!(e["title"].as_str().unwrap().chars().count() <= EMBED_TITLE_MAX);
    }

    #[test]
    fn a_card_with_controls_carries_one_row_and_one_without_carries_none() {
        use crate::messaging::buttons::Controls;
        let mut c = card(None, None);
        assert!(now_playing(&c, &RenderCx::default()).rows.is_empty());
        c.rows = Controls {
            guild: serenity::all::GuildId::new(1).to_ref(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: false,
        }
        .rows();
        let r = now_playing(&c, &RenderCx::default());
        assert_eq!(r.rows.len(), 1);
        let row =
            serde_json::to_value(&crate::messaging::discord::action_rows(&r.rows)[0]).unwrap();
        assert_eq!(row["components"][0]["custom_id"], "np:pause:1");
    }

    /// The ids name a Discord server: a guild that is no snowflake has none.
    #[test]
    fn a_non_snowflake_guild_gets_no_buttons() {
        use crate::messaging::buttons::Controls;
        let mut c = card(None, None);
        c.rows = Controls {
            guild: crack_model::CommunityRef::new(crack_model::Platform::Discord, "0").unwrap(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: false,
        }
        .rows();
        let with = now_playing(&c, &RenderCx::default());
        assert!(with.rows.is_empty());
        c.rows = Vec::new();
        let without = now_playing(&c, &RenderCx::default());
        assert_eq!(with.cards, without.cards);
        assert_eq!(with.rows, without.rows);
    }

    /// "None on Finished": the edit to Finished clears the row.
    #[test]
    fn the_finished_card_has_no_buttons() {
        assert!(finished().rows.is_empty());
        let edit = serde_json::to_value(finished().to_edit()).unwrap();
        assert_eq!(edit["components"].as_array().map(Vec::len), Some(0));
    }

    /// The Discord bytes of each music message, captured before the cards
    /// were built neutral. Only the payloads' construction may change.
    #[test]
    fn music_cards_render_as_before() {
        use crate::messaging::buttons::Controls;
        use crate::messaging::message::CrackedMessage;
        use crate::messaging::render::render;
        use crate::messaging::track_failed::{FailReason, Failure};

        fn bytes(r: &Rendered) -> (String, String, String) {
            (
                serde_json::to_string(&crate::messaging::discord::first_embed(r)).unwrap(),
                serde_json::to_string(&crate::messaging::discord::action_rows(&r.rows)).unwrap(),
                serde_json::to_string(&crate::messaging::discord::extra_embeds(r)).unwrap(),
            )
        }
        fn pin(r: &Rendered, embed: &str, components: &str) {
            assert_eq!(
                bytes(r),
                (embed.to_owned(), components.to_owned(), "[]".to_owned())
            );
        }
        let cx = RenderCx {
            now_unix: 1_000_000,
            embed_links: true,
        };

        let mut playing = card(
            Some("https://www.youtube.com/watch?v=x"),
            Some("https://i.ytimg.com/a.jpg"),
        );
        playing.rows = Controls {
            guild: serenity::all::GuildId::new(7).to_ref(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: true,
        }
        .rows();
        pin(&now_playing(&playing, &cx), "{\"title\":\"OH SHIT I'M FEELING IT\",\"type\":\"rich\",\"description\":\"[vote](https://top.gg/bot/1115229568006103122/vote) • [invite](https://discord.com/oauth2/authorize?client_id=1115229568006103122&permissions=551940115520&scope=bot+applications.commands)\",\"url\":\"https://www.youtube.com/watch?v=x\",\"footer\":{\"text\":\"Streaming via youtube.com\",\"icon_url\":\"https://www.google.com/s2/favicons?domain=youtube.com\"},\"thumbnail\":{\"url\":\"https://i.ytimg.com/a.jpg\",\"description\":null},\"author\":{\"name\":\"🔊 Now playing\",\"url\":null,\"icon_url\":null},\"fields\":[{\"name\":\"Progress\",\"value\":\">>> 4:33 · ends <t:1000200:R>\",\"inline\":true},{\"name\":\"Requested by\",\"value\":\">>> <@42>\",\"inline\":true}]}",
            "[{\"type\":1,\"components\":[{\"style\":2,\"type\":2,\"custom_id\":\"np:pause:7\",\"emoji\":{\"name\":\"⏸\u{fe0f}\"},\"disabled\":false},{\"style\":2,\"type\":2,\"custom_id\":\"np:skip:7:00000000-0000-0000-0000-000000000000\",\"emoji\":{\"name\":\"⏭\u{fe0f}\"},\"disabled\":false},{\"style\":3,\"type\":2,\"custom_id\":\"np:repeat-off:7\",\"emoji\":{\"name\":\"🔁\"},\"disabled\":false},{\"style\":2,\"type\":2,\"custom_id\":\"np:shuffle:7\",\"emoji\":{\"name\":\"🔀\"},\"disabled\":false}]}]");

        pin(
            &queued(
                &queued_card(
                    Some(Duration::from_secs(273)),
                    Some(Duration::from_secs(48)),
                ),
                &cx,
            ),
            "{\"title\":\"OH SHIT I'M FEELING IT\",\"type\":\"rich\",\"url\":\"https://www.youtube.com/watch?v=x\",\"footer\":{\"text\":\"Track duration: 4:33\\nEstimated time until play: 0:48\",\"icon_url\":null},\"thumbnail\":{\"url\":\"https://i.ytimg.com/a.jpg\",\"description\":null},\"author\":{\"name\":\"📃 Added to queue!\",\"url\":null,\"icon_url\":null}}",
            "[]",
        );

        let line = EchoLine {
            echo: Echo::Skipped {
                title: Some("A*B".into()),
            },
            user: UserId::new(42).to_ref(),
            via: Via::Dashboard,
        };
        pin(&echo(&line), "{\"type\":\"rich\",\"description\":\"⏭ Skipped **A\\\\*B** from the dashboard — <@42>\"}",
            "[]");
        pin(&finished(), "{\"title\":\"⏹\u{fe0f} Finished\",\"type\":\"rich\",\"description\":\"Nothing is playing. Use /play to start again.\"}",
            "[]");

        let failed = CrackedMessage::TrackFailed {
            listed: vec![
                Failure {
                    title: Some("one".into()),
                    reason: FailReason::Open,
                },
                Failure {
                    title: None,
                    reason: FailReason::Seek,
                },
            ],
            more: 3,
        };
        pin(&render(&failed, &cx), "{\"type\":\"rich\",\"description\":\"⚠\u{fe0f} Couldn't play 5 tracks:\\n• **one**: couldn't open the stream\\n• **(untitled)**: couldn't seek in it\\n+3 more\"}",
            "[]");

        pin(&render(&CrackedMessage::Error, &cx), "{\"type\":\"rich\",\"description\":\"Fatality! Something went wrong ☹\u{fe0f}\",\"color\":15158332}",
            "[]");
    }

    /// Autoplay is stored as user 1; the card says so instead of mentioning it.
    #[test]
    fn an_autoplayed_card_names_the_requester_auto() {
        let mut c = card(None, None);
        c.requester = Some(UserId::new(1).to_ref());
        let out = now_playing(&c, &RenderCx::default());
        assert_eq!(out.cards[0].fields[1].value, ">>> (auto)");
    }
}
