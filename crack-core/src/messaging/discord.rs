//! The poise half of the Discord adapter: [`ToPoiseReply`] turns a
//! [`Rendered`] into the `poise::CreateReply` a command sends. Everything
//! else lives in crack-common's `discord` and is re-exported here.

pub use crack_common::discord::{action_rows, content, embed, embeds, ToDiscord};
#[cfg(test)]
pub use crack_common::discord::{extra_embeds, first_embed};

use crack_model::Rendered;

/// A [`Rendered`] as the poise reply for each way a command answers.
pub trait ToPoiseReply {
    /// Every command reply is a reply-reference to the invoking message
    /// (`.reply(true)`): no caller ever asked for a bare message, and poise
    /// ignores the flag for slash commands. The flag is private on
    /// `poise::CreateReply`, so no test can observe it; this line is deliberate.
    fn to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static>;
    /// The reply for `ReplyHandle::edit`, which ties the builder's lifetime to
    /// the context's. `CreateReply` and `CreateComponent` are invariant in it,
    /// so a `'static` component list cannot be passed: it is left out, and the
    /// caller is told so by the returned flag.
    ///
    /// 🪤 Unlike [`ToDiscord::to_edit`], this does not clear absent content:
    /// poise sets the content only when it is `Some`, so an edit without
    /// content leaves the old text on the message. (Embeds it always
    /// replaces, an empty list included.)
    fn to_reply_edit<'a>(&self) -> (poise::CreateReply<'a>, bool);
}

impl ToPoiseReply for Rendered {
    fn to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static> {
        let mut reply = poise::CreateReply::default()
            .reply(true)
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(action_rows(&self.rows));
        if let Some(content) = content(self) {
            reply = reply.content(content);
        }
        for e in embeds(self) {
            reply = reply.embed(e);
        }
        reply
    }

    fn to_reply_edit<'a>(&self) -> (poise::CreateReply<'a>, bool) {
        let mut reply: poise::CreateReply<'a> =
            poise::CreateReply::default().allowed_mentions(self.allowed_mentions());
        if let Some(content) = content(self) {
            reply = reply.content(content);
        }
        for e in embeds(self) {
            reply = reply.embed(e);
        }
        (reply, !self.rows.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_common::format::CONTENT_MAX;
    use crack_model::{Button, ButtonStyle, Row};
    use serenity::all::{CreateInteractionResponseMessage, EditMessage};

    fn json<T: serde::Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    fn btn(
        id: &str,
        label: Option<&str>,
        emoji: Option<&str>,
        style: ButtonStyle,
        disabled: bool,
    ) -> Button {
        Button {
            id: id.into(),
            label: label.map(Into::into),
            emoji: emoji.map(Into::into),
            style,
            disabled,
        }
    }

    #[test]
    fn a_reply_edit_says_whether_it_carries_rows() {
        assert!(!Rendered::text("x").to_reply_edit().1);
        let rows = vec![Row::Buttons(vec![btn(
            "a",
            None,
            None,
            ButtonStyle::Primary,
            false,
        )])];
        assert!(Rendered::text("x").with_rows(rows).to_reply_edit().1);
    }

    fn long() -> Rendered {
        Rendered::text("a".repeat(3000))
    }

    /// The `content` of a serialised builder, as a character count.
    fn content_len<T: serde::Serialize>(v: &T) -> usize {
        #[derive(serde::Deserialize)]
        struct Body {
            content: String,
        }
        let body: Body = serde_json::from_str(&json(v)).unwrap();
        body.content.chars().count()
    }

    #[test]
    fn every_send_path_clips_long_content() {
        let r = long();
        let reply = r
            .to_reply(false)
            .to_slash_initial_response(CreateInteractionResponseMessage::new());
        assert_eq!(content_len(&reply), CONTENT_MAX);
        let reply_edit = r.to_reply_edit().0.to_prefix_edit(EditMessage::new());
        assert_eq!(content_len(&reply_edit), CONTENT_MAX);
    }

    #[test]
    fn mentions_none_survives_every_send_path() {
        let r = Rendered::text("<@123>");
        let none = "\"allowed_mentions\":{\"parse\":[],\"users\":[],\"roles\":[]}";
        for wire in [
            json(
                &r.to_reply(false)
                    .to_slash_initial_response(CreateInteractionResponseMessage::new()),
            ),
            json(&r.to_reply_edit().0.to_prefix_edit(EditMessage::new())),
        ] {
            assert!(wire.contains(none), "{wire}");
        }
    }
}
