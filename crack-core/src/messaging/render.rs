//! What a [`CrackedMessage`] looks like in Discord.
//!
//! `render` is pure and total: every message renders, nothing here touches
//! the network, and it is the only sender-facing way to turn a message into
//! Discord output. Titles, links, durations and limits come from
//! [`super::format`]; nothing else formats them.
use crate::messaging::format::{clip, CONTENT_MAX, DESCRIPTION_MAX};
use crate::messaging::message::CrackedMessage;
use serenity::all::{
    Colour, CreateAllowedMentions, CreateComponent, CreateEmbed, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateMessage, EditMessage,
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
    /// Embeds after `embed`, in order, for the one message that shows a list
    /// of them (search results).
    pub embeds_extra: Vec<CreateEmbed<'static>>,
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

    /// `embed`, then `embeds_extra`: every embed the message carries, in order.
    fn embeds(&self) -> Vec<CreateEmbed<'static>> {
        self.embed
            .iter()
            .chain(&self.embeds_extra)
            .cloned()
            .collect()
    }

    pub fn allowed_mentions(&self) -> CreateAllowedMentions<'static> {
        match self.mentions {
            Mentions::None => CreateAllowedMentions::new(),
            Mentions::Users => CreateAllowedMentions::new().all_users(true),
        }
    }

    /// Every command reply is a reply-reference to the invoking message
    /// (`.reply(true)`): no caller ever asked for a bare message, and poise
    /// ignores the flag for slash commands. The flag is private on
    /// `poise::CreateReply`, so no test can observe it; this line is deliberate.
    #[must_use]
    pub fn to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static> {
        let mut reply = poise::CreateReply::default()
            .reply(true)
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            reply = reply.content(content.clone());
        }
        for embed in self.embeds() {
            reply = reply.embed(embed);
        }
        reply
    }

    /// The reply for `ReplyHandle::edit`, which ties the builder's lifetime to
    /// the context's. `CreateReply` and `CreateComponent` are invariant in it,
    /// so a `'static` component list cannot be passed: it is left out, and the
    /// caller is told so by the returned flag.
    ///
    /// 🪤 Unlike [`Rendered::to_edit`], this does not clear absent content:
    /// poise sets the content only when it is `Some`, so an edit without
    /// content leaves the old text on the message. (Embeds it always
    /// replaces, an empty list included.)
    #[must_use]
    pub fn to_reply_edit<'a>(&self) -> (poise::CreateReply<'a>, bool) {
        let mut reply: poise::CreateReply<'a> =
            poise::CreateReply::default().allowed_mentions(self.allowed_mentions());
        if let Some(content) = &self.content {
            reply = reply.content(content.clone());
        }
        for embed in self.embeds() {
            reply = reply.embed(embed);
        }
        (reply, !self.components.is_empty())
    }

    pub fn to_message(&self) -> CreateMessage<'static> {
        let mut m = CreateMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            m = m.content(content.clone());
        }
        let embeds = self.embeds();
        if !embeds.is_empty() {
            m = m.embeds(embeds);
        }
        m
    }

    /// The body of a component interaction's `UpdateMessage` response (a page
    /// flip). The response itself stays with the caller until interaction
    /// responses move here.
    pub fn to_interaction_message(&self) -> CreateInteractionResponseMessage<'static> {
        let mut m = CreateInteractionResponseMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            m = m.content(content.clone());
        }
        let embeds = self.embeds();
        if !embeds.is_empty() {
            m = m.embeds(embeds);
        }
        m
    }

    /// A follow-up to a component interaction (a button press's answer).
    pub fn to_followup(&self, ephemeral: bool) -> CreateInteractionResponseFollowup<'static> {
        let mut f = CreateInteractionResponseFollowup::new()
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone());
        if let Some(content) = &self.content {
            f = f.content(content.clone());
        }
        let embeds = self.embeds();
        if !embeds.is_empty() {
            f = f.embeds(embeds);
        }
        f
    }

    /// An edit replaces everything: a field left `None` is cleared, so a
    /// status that loses its buttons really loses them.
    pub fn to_edit(&self) -> EditMessage<'static> {
        EditMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(self.components.clone())
            .content(self.content.clone().unwrap_or_default())
            .embeds(self.embeds())
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
        CrackedMessage::Gp(card) => crate::commands::music::gp::render_card(card, cx),
        CrackedMessage::TrackFailed { listed, more } => {
            Rendered::embed(CreateEmbed::new().description(clip(
                &super::track_failed::render_text(listed, *more),
                DESCRIPTION_MAX,
            )))
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::TrackLabel;
    use crate::messaging::message::CrackedMessage;

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    #[test]
    fn text_is_clipped_to_the_content_limit() {
        let out = Rendered::text("a".repeat(3000));
        assert_eq!(out.content.unwrap().chars().count(), CONTENT_MAX);
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
        assert!(r.embed.is_none());
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
        let r = Rendered::embed(CreateEmbed::new().title("Page 2"))
            .with_components(crate::messaging::interface::create_nav_btns(1, 3));
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
        let mut r = Rendered::embed(CreateEmbed::new().title("a"));
        r.embeds_extra = vec![CreateEmbed::new().title("b"), CreateEmbed::new().title("c")];
        let abc = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

        let sent = serde_json::to_value(r.to_message()).unwrap();
        assert_eq!(embed_titles(&sent), abc);
        let edited = serde_json::to_value(r.to_edit()).unwrap();
        assert_eq!(embed_titles(&edited), abc);
        let reply = r
            .to_reply(false)
            .to_slash_initial_response(serenity::all::CreateInteractionResponseMessage::new());
        assert_eq!(embed_titles(&serde_json::to_value(reply).unwrap()), abc);
        let reply_edit = r.to_reply_edit().0.to_prefix_edit(EditMessage::new());
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
        let v = serde_json::to_value(r.embed.unwrap()).unwrap();
        assert_eq!(v["color"], serde_json::json!(serenity::all::Colour::RED.0));
    }
}
