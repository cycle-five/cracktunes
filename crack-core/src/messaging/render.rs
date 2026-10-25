//! What a [`CrackedMessage`] looks like, as platform-neutral output.
//!
//! `render` is pure and total: every message renders, nothing here touches
//! the network, and it is the only sender-facing way to turn a message into
//! output; `messaging::discord` converts that to Discord builders. Titles,
//! links, durations and limits come from [`super::format`]; nothing else
//! formats them.
use crate::messaging::format::{clip, DESCRIPTION_MAX};
use crate::messaging::message::CrackedMessage;
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

/// A feature's own message type: it displays as a log line and renders itself.
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
                &super::track_failed::render_text(listed, *more),
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

/// The embed description of `r`, for tests.
#[cfg(test)]
#[must_use]
pub fn description(r: &Rendered) -> Option<String> {
    r.cards.first()?.description.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::discord::ToDiscord;
    use crate::messaging::format::TrackLabel;
    use crate::messaging::message::CrackedMessage;

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    /// An edit replaces everything: a message edited to one without an embed
    /// must lose its old embed and content.
    #[test]
    fn an_edit_without_an_embed_clears_the_old_one() {
        let json = serde_json::to_value(Rendered::default().to_edit()).expect("serializes");
        assert_eq!(json["embeds"], serde_json::json!([]));
        assert_eq!(json["content"], serde_json::json!(""));
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

    /// No message is `Style::Text` today (see `CrackedMessage::style`), so
    /// the text path is pinned through `Rendered::text` itself.
    #[test]
    fn text_renders_as_content_and_not_as_an_embed() {
        let r = Rendered::text("pong");
        assert!(r.cards.is_empty());
        assert_eq!(r.content.as_deref(), Some("pong"));
    }

    /// A follow-up to a press: private when asked, pings nobody, carries the embed.
    #[test]
    fn a_followup_is_private_when_asked_and_pings_nobody() {
        let r = render(&CrackedMessage::Other("@everyone".into()), &cx());
        let private = serde_json::to_value(r.to_followup(true)).unwrap();
        assert_eq!(private["flags"].as_u64(), Some(64));
        assert_eq!(private["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(private["embeds"].as_array().map(Vec::len), Some(1));
        let public = serde_json::to_value(r.to_followup(false)).unwrap();
        assert_eq!(public["flags"].as_u64().unwrap_or(0) & 64, 0);
    }

    /// A page flip carries the new embed and the nav buttons, and pings nobody.
    #[test]
    fn an_interaction_update_carries_the_embed_and_the_buttons() {
        let r = Rendered::card(Card {
            title: Some("Page 2".into()),
            ..Card::default()
        })
        .with_rows(crate::messaging::interface::create_nav_btns(1, 3));
        let v = serde_json::to_value(r.to_interaction_message()).unwrap();
        assert_eq!(v["embeds"][0]["title"], "Page 2");
        assert_eq!(v["components"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["allowed_mentions"]["parse"], serde_json::json!([]));
    }

    fn embed_titles(v: &serde_json::Value) -> Vec<String> {
        v["embeds"]
            .as_array()
            .expect("embeds")
            .iter()
            .map(|e| e["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Search results are one message with an embed per hit: every send and
    /// edit carries all of them, in order.
    #[test]
    fn extra_embeds_follow_the_embed_everywhere() {
        let titled = |t: &str| Card {
            title: Some(t.into()),
            ..Card::default()
        };
        let r = Rendered::card(titled("a")).with_cards(vec![titled("b"), titled("c")]);
        let abc = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

        let sent = serde_json::to_value(r.to_message()).unwrap();
        assert_eq!(embed_titles(&sent), abc);
        let edited = serde_json::to_value(r.to_edit()).unwrap();
        assert_eq!(embed_titles(&edited), abc);
        let reply = r
            .to_reply(false)
            .to_slash_initial_response(serenity::all::CreateInteractionResponseMessage::new());
        assert_eq!(embed_titles(&serde_json::to_value(reply).unwrap()), abc);
        let reply_edit = r
            .to_reply_edit()
            .0
            .to_prefix_edit(serenity::all::EditMessage::new());
        assert_eq!(
            embed_titles(&serde_json::to_value(reply_edit).unwrap()),
            abc
        );
    }

    /// The flag `to_reply` is given is the one Discord gets, set either way:
    /// poise fills in a command's `ephemeral` attribute only when a reply
    /// leaves it unset, so `false` makes even an `ephemeral` command public.
    #[test]
    fn a_replys_ephemeral_flag_reaches_the_wire() {
        let flags = |ephemeral: bool| {
            let r = Rendered::text("x")
                .to_reply(ephemeral)
                .to_slash_initial_response(serenity::all::CreateInteractionResponseMessage::new());
            serde_json::to_value(r).unwrap()["flags"].as_u64()
        };
        assert_eq!(flags(true), Some(64));
        assert_eq!(flags(false).unwrap_or(0) & 64, 0);
    }

    #[test]
    fn errors_are_red() {
        let r = render(&CrackedMessage::Error, &cx());
        let v = serde_json::to_value(crate::messaging::discord::embed(&r.cards[0])).unwrap();
        assert_eq!(v["color"], serde_json::json!(serenity::all::Colour::RED.0));
    }
}
