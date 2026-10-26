//! What a [`CrackedMessage`] looks like, as platform-neutral output.
//!
//! `render` is pure and total: every message renders, nothing here touches
//! the network, and it is the only sender-facing way to turn a message into
//! output; crack-core's `messaging::discord` converts that to Discord builders. Titles,
//! links, durations and limits come from [`super::format`]; nothing else
//! formats them.
use crate::format::{clip, DESCRIPTION_MAX};
use crate::message::CrackedMessage;
use std::time::{SystemTime, UNIX_EPOCH};

use crack_model::Card;
pub use crack_model::{Mentions, Rendered};

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

/// A feature's own message type: it displays as text (user-visible for some
/// payloads, e.g. `GpCard::Started`, so don't tidy it as a log line) and renders itself.
/// `CrackedMessage` holds one per feature, so it names none of them.
pub trait Payload: std::fmt::Debug + std::fmt::Display + Send + Sync {
    fn render(&self, cx: &RenderCx) -> Rendered;
}

/// Render `msg`. Pure and total.
#[must_use]
pub fn render(msg: &CrackedMessage, cx: &RenderCx) -> Rendered {
    match msg {
        CrackedMessage::Card(card) => Rendered::card((**card).clone()),
        CrackedMessage::NowPlayingCard(card) => super::cards::now_playing(card, cx),
        CrackedMessage::Finished => super::cards::finished(),
        CrackedMessage::Queued(card) => super::cards::queued(card, cx),
        CrackedMessage::Echo(line) => super::cards::echo(line),
        CrackedMessage::Gp(p) | CrackedMessage::Room(p) | CrackedMessage::ModLog(p) => p.render(cx),
        CrackedMessage::TrackFailed { listed, more } => Rendered::card(Card {
            description: Some(clip(
                &super::failure::render_text(listed, *more),
                DESCRIPTION_MAX,
            )),
            ..Card::default()
        }),
        other => {
            let text = other.to_string();
            match other.style() {
                Style::Text => Rendered::text(text),
                Style::Embed => Rendered::card(Card {
                    description: Some(clip(&text, DESCRIPTION_MAX)),
                    colour: Some(other.colour()),
                    ..Card::default()
                }),
            }
        },
    }
}

/// The embed description of `r`, for tests (crack-core's use it too, so it is not `cfg(test)`).
#[must_use]
pub fn description(r: &Rendered) -> Option<String> {
    r.cards.first()?.description.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::TrackLabel;
    use crate::message::CrackedMessage;
    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    /// The owner's TuneTitan screenshot, 2026-10-06: "⏭️ Skipped to **!".
    #[test]
    fn skipping_to_an_untitled_track_names_it_untitled() {
        let r = render(&CrackedMessage::SkipTo(TrackLabel::default()), &cx());
        assert_eq!(
            description(&r).as_deref(),
            Some("⏭️ Skipped to **(untitled)**!")
        );
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
    fn an_overlong_message_is_clipped_to_discords_limit() {
        let long = "x".repeat(5000);
        let r = render(&CrackedMessage::Other(long), &cx());
        assert_eq!(description(&r).unwrap().chars().count(), 4096);
    }

    /// No message is `Style::Text` today (see `CrackedMessage::style`), so
    /// the text path is pinned through `Rendered::text` itself.
    #[test]
    fn text_renders_as_content_and_not_as_an_embed() {
        let r = Rendered::text("pong");
        assert!(r.cards.is_empty());
        assert_eq!(r.content.as_deref(), Some("pong"));
    }

    #[test]
    fn finished_renders_its_card() {
        let r = render(&CrackedMessage::Finished, &cx());
        assert_eq!(r.cards.len(), 1);
        assert_eq!(r.cards[0].title.as_deref(), Some("⏹️ Finished"));
        assert_eq!(
            r.cards[0].description.as_deref(),
            Some("Nothing is playing. Use /play to start again.")
        );
    }

    #[test]
    fn an_echo_renders_its_line_as_the_description() {
        use crate::cards::{EchoLine, Via};
        use crack_types::discord::ToRef;
        let line = EchoLine {
            echo: crate::cards::Echo::Paused,
            user: serenity::all::UserId::new(42).to_ref(),
            via: Via::Button,
        };
        let r = render(&CrackedMessage::Echo(Box::new(line)), &cx());
        assert_eq!(description(&r).as_deref(), Some("⏸ Paused — <@42>"));
    }

    #[test]
    fn a_queued_message_renders_the_queued_card() {
        use crate::cards::QueuedCard;
        let card = QueuedCard {
            author: "Added to queue",
            label: TrackLabel {
                title: Some("Waterloo".into()),
                url: None,
                duration: None,
            },
            thumbnail: None,
            wait: None,
        };
        let r = render(&CrackedMessage::Queued(Box::new(card)), &cx());
        assert_eq!(r.cards.len(), 1);
        assert_eq!(r.cards[0].title.as_deref(), Some("Waterloo"));
        assert_eq!(
            r.cards[0].author.as_ref().map(|a| a.name.as_str()),
            Some("Added to queue")
        );
    }

    #[test]
    fn the_now_playing_arm_renders_the_card_with_its_progress() {
        use crate::cards::NowPlayingCard;
        use crate::format::Progress;
        use crack_types::discord::ToRef;
        use std::time::Duration;
        let card = NowPlayingCard {
            label: TrackLabel {
                title: Some("Waterloo".into()),
                url: Some("https://www.youtube.com/watch?v=x".into()),
                duration: Some(Duration::from_secs(273)),
            },
            thumbnail: None,
            requester: Some(serenity::all::UserId::new(42).to_ref()),
            progress: Progress::Paused {
                position: Some(Duration::from_secs(72)),
            },
            rows: Vec::new(),
        };
        let r = render(&CrackedMessage::NowPlayingCard(Box::new(card)), &cx());
        let c = &r.cards[0];
        assert_eq!(c.title.as_deref(), Some("Waterloo"));
        assert_eq!(c.url.as_deref(), Some("https://www.youtube.com/watch?v=x"));
        assert_eq!(c.fields[0].value, ">>> Paused at 1:12");
        assert_eq!(c.fields[1].value, ">>> <@42>");
        assert_eq!(
            c.footer.as_ref().map(|f| f.text.as_str()),
            Some("Streaming via youtube.com")
        );
        assert!(r.rows.is_empty());
    }

    #[test]
    fn a_track_failed_message_renders_the_text_as_its_description() {
        use crate::failure::{FailReason, Failure};
        let one = Failure {
            title: Some("A".into()),
            reason: FailReason::Open,
        };
        let r = render(
            &CrackedMessage::TrackFailed {
                listed: vec![one.clone()],
                more: 2,
            },
            &cx(),
        );
        assert_eq!(
            description(&r).as_deref(),
            Some("⚠️ Couldn't play 3 tracks:\n• **A**: couldn't open the stream\n+2 more")
        );
        let many = vec![one; 200];
        let r = render(
            &CrackedMessage::TrackFailed {
                listed: many,
                more: 0,
            },
            &cx(),
        );
        assert_eq!(description(&r).unwrap().chars().count(), 4096);
    }
}
