//! The Discord adapter for the neutral output types: each function turns one
//! of them into the serenity builder that sends the same thing today.

// This file is where neutral output becomes Discord builders.
#![allow(clippy::disallowed_types)]

use std::borrow::Cow;

use crack_model::{Button, ButtonStyle, Card, Mentions, Rendered, Row, Select, SelectOption};
use serenity::all::{
    ButtonStyle as DiscordStyle, Colour, CreateActionRow, CreateAllowedMentions, CreateButton,
    CreateComponent, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
    CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, EditMessage, ReactionType,
    Timestamp,
};

use super::format::{clip, CONTENT_MAX};
use serenity::small_fixed_array::FixedString;

/// A card as an embed. A timestamp serenity cannot represent is dropped.
pub fn embed(card: &Card) -> CreateEmbed<'static> {
    let mut e = CreateEmbed::new();
    if let Some(title) = &card.title {
        e = e.title(title.clone());
    }
    if let Some(url) = &card.url {
        e = e.url(url.clone());
    }
    if let Some(description) = &card.description {
        e = e.description(description.clone());
    }
    if let Some(author) = &card.author {
        let mut a = CreateEmbedAuthor::new(author.name.clone());
        if let Some(url) = &author.url {
            a = a.url(url.clone());
        }
        if let Some(icon) = &author.icon_url {
            a = a.icon_url(icon.clone());
        }
        e = e.author(a);
    }
    for f in &card.fields {
        e = e.field(f.name.clone(), f.value.clone(), f.inline);
    }
    if let Some(thumbnail) = &card.thumbnail {
        e = e.thumbnail(thumbnail.clone(), None);
    }
    if let Some(footer) = &card.footer {
        let mut f = CreateEmbedFooter::new(footer.text.clone());
        if let Some(icon) = &footer.icon_url {
            f = f.icon_url(icon.clone());
        }
        e = e.footer(f);
    }
    if let Some(ts) = card
        .timestamp
        // serenity multiplies by 1000 unchecked, which panics in debug builds.
        .filter(|secs| secs.checked_mul(1000).is_some())
        .and_then(|secs| Timestamp::from_unix_timestamp(secs).ok())
    {
        e = e.timestamp(ts);
    }
    if let Some(colour) = card.colour {
        e = e.colour(Colour::new(colour));
    }
    e
}

/// Each row as one action row, in order.
pub fn action_rows(rows: &[Row]) -> Vec<CreateComponent<'static>> {
    rows.iter().map(action_row).collect()
}

fn action_row(row: &Row) -> CreateComponent<'static> {
    CreateComponent::ActionRow(match row {
        Row::Buttons(buttons) => {
            CreateActionRow::Buttons(Cow::Owned(buttons.iter().map(button).collect::<Vec<_>>()))
        },
        Row::Select(select) => CreateActionRow::SelectMenu(menu(select)),
    })
}

fn button(b: &Button) -> CreateButton<'static> {
    let mut out = CreateButton::new(b.id.clone())
        .style(button_style(b.style))
        .disabled(b.disabled);
    if let Some(label) = &b.label {
        out = out.label(label.clone());
    }
    if let Some(emoji) = &b.emoji {
        out = out.emoji(ReactionType::Unicode(FixedString::from_string_trunc(
            emoji.clone(),
        )));
    }
    out
}

fn menu(s: &Select) -> CreateSelectMenu<'static> {
    let options: Vec<CreateSelectMenuOption<'static>> =
        s.options.iter().map(select_option).collect();
    let mut m = CreateSelectMenu::new(
        s.id.clone(),
        CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        },
    );
    if let Some(p) = &s.placeholder {
        m = m.placeholder(p.clone());
    }
    if let Some(min) = s.min {
        m = m.min_values(min);
    }
    if let Some(max) = s.max {
        m = m.max_values(max);
    }
    m
}

fn select_option(o: &SelectOption) -> CreateSelectMenuOption<'static> {
    let option = CreateSelectMenuOption::new(o.label.clone(), o.value.clone());
    // Absent, not `false`: the menus sent today never state it.
    if o.default {
        option.default_selection(true)
    } else {
        option
    }
}

fn button_style(s: ButtonStyle) -> DiscordStyle {
    match s {
        ButtonStyle::Primary => DiscordStyle::Primary,
        ButtonStyle::Secondary => DiscordStyle::Secondary,
        ButtonStyle::Success => DiscordStyle::Success,
        ButtonStyle::Danger => DiscordStyle::Danger,
    }
}

/// Who a message may ping: nobody, or the users its text mentions.
pub fn allowed_mentions(m: Mentions) -> CreateAllowedMentions<'static> {
    match m {
        Mentions::None => CreateAllowedMentions::new(),
        Mentions::Users => CreateAllowedMentions::new().all_users(true),
    }
}

/// A [`Rendered`] as the Discord builder for each way it is sent.
///
/// Every conversion clips `content` to Discord's limit: this is the one place
/// the limit is applied, so no send path can skip it.
pub trait ToDiscord {
    fn allowed_mentions(&self) -> CreateAllowedMentions<'static>;
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
    fn to_message(&self) -> CreateMessage<'static>;
    /// The body of a component interaction's `UpdateMessage` response (a page
    /// flip). The response itself stays with the caller.
    fn to_interaction_message(&self) -> CreateInteractionResponseMessage<'static>;
    /// A follow-up to a component interaction (a button press's answer).
    fn to_followup(&self, ephemeral: bool) -> CreateInteractionResponseFollowup<'static>;
    /// An edit replaces everything: a field left empty is cleared, so a
    /// status that loses its buttons really loses them.
    fn to_edit(&self) -> EditMessage<'static>;
}

fn content(r: &Rendered) -> Option<String> {
    r.content.as_deref().map(|c| clip(c, CONTENT_MAX))
}

fn embeds(r: &Rendered) -> Vec<CreateEmbed<'static>> {
    r.cards.iter().map(embed).collect()
}

/// The first card as an embed, for tests that read what Discord would get.
#[cfg(test)]
pub fn first_embed(r: &Rendered) -> Option<CreateEmbed<'static>> {
    r.cards.first().map(embed)
}

/// Every card after the first, as embeds, for tests.
#[cfg(test)]
pub fn extra_embeds(r: &Rendered) -> Vec<CreateEmbed<'static>> {
    r.cards.iter().skip(1).map(embed).collect()
}

impl ToDiscord for Rendered {
    fn allowed_mentions(&self) -> CreateAllowedMentions<'static> {
        allowed_mentions(self.mentions)
    }

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

    fn to_message(&self) -> CreateMessage<'static> {
        let mut m = CreateMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(action_rows(&self.rows));
        if let Some(content) = content(self) {
            m = m.content(content);
        }
        let embeds = embeds(self);
        if !embeds.is_empty() {
            m = m.embeds(embeds);
        }
        m
    }

    fn to_interaction_message(&self) -> CreateInteractionResponseMessage<'static> {
        let mut m = CreateInteractionResponseMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(action_rows(&self.rows));
        if let Some(content) = content(self) {
            m = m.content(content);
        }
        let embeds = embeds(self);
        if !embeds.is_empty() {
            m = m.embeds(embeds);
        }
        m
    }

    fn to_followup(&self, ephemeral: bool) -> CreateInteractionResponseFollowup<'static> {
        let mut f = CreateInteractionResponseFollowup::new()
            .ephemeral(ephemeral)
            .allowed_mentions(self.allowed_mentions())
            .components(action_rows(&self.rows));
        if let Some(content) = content(self) {
            f = f.content(content);
        }
        let embeds = embeds(self);
        if !embeds.is_empty() {
            f = f.embeds(embeds);
        }
        f
    }

    fn to_edit(&self) -> EditMessage<'static> {
        EditMessage::new()
            .allowed_mentions(self.allowed_mentions())
            .components(action_rows(&self.rows))
            .content(content(self).unwrap_or_default())
            .embeds(embeds(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_model::{Author, Field, Footer};

    fn json<T: serde::Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    #[test]
    fn every_card_field_converts_like_the_builder() {
        let card = Card {
            title: Some("T".into()),
            url: Some("https://e.x/a".into()),
            description: Some("D".into()),
            author: Some(Author {
                name: "A".into(),
                url: Some("https://e.x/u".into()),
                icon_url: Some("https://e.x/i.png".into()),
            }),
            fields: vec![
                Field {
                    name: "n1".into(),
                    value: "v1".into(),
                    inline: true,
                },
                Field {
                    name: "n2".into(),
                    value: "v2".into(),
                    inline: false,
                },
            ],
            thumbnail: Some("https://e.x/t.png".into()),
            footer: Some(Footer {
                text: "F".into(),
                icon_url: Some("https://e.x/f.png".into()),
            }),
            timestamp: Some(1_700_000_000),
            colour: Some(0x00AB_CDEF),
        };
        let old = CreateEmbed::new()
            .title("T")
            .url("https://e.x/a")
            .description("D")
            .author(
                CreateEmbedAuthor::new("A")
                    .url("https://e.x/u")
                    .icon_url("https://e.x/i.png"),
            )
            .field("n1", "v1", true)
            .field("n2", "v2", false)
            .thumbnail("https://e.x/t.png", None)
            .footer(CreateEmbedFooter::new("F").icon_url("https://e.x/f.png"))
            .timestamp(Timestamp::from_unix_timestamp(1_700_000_000).unwrap())
            .colour(Colour::new(0x00AB_CDEF));
        assert_eq!(json(&embed(&card)), json(&old));

        let bare = Card {
            author: Some(Author {
                name: "A".into(),
                url: None,
                icon_url: None,
            }),
            footer: Some(Footer {
                text: "F".into(),
                icon_url: None,
            }),
            ..Card::default()
        };
        let old = CreateEmbed::new()
            .author(CreateEmbedAuthor::new("A"))
            .footer(CreateEmbedFooter::new("F"));
        assert_eq!(json(&embed(&bare)), json(&old));

        assert_eq!(json(&embed(&Card::default())), json(&CreateEmbed::new()));
        let bad_ts = Card {
            timestamp: Some(i64::MAX),
            ..Card::default()
        };
        assert_eq!(json(&embed(&bad_ts)), json(&CreateEmbed::new()));
        let out_of_range = Card {
            timestamp: Some(1_000_000_000_000_000),
            ..Card::default()
        };
        assert_eq!(json(&embed(&out_of_range)), json(&CreateEmbed::new()));
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
    fn every_control_converts_like_the_builder() {
        let rows = vec![
            Row::Buttons(vec![
                btn("a", Some("L"), Some("👍"), ButtonStyle::Primary, false),
                btn("b", None, Some("🔁"), ButtonStyle::Secondary, true),
                btn("c", Some("C"), None, ButtonStyle::Success, false),
                btn("d", None, None, ButtonStyle::Danger, true),
            ]),
            Row::Select(Select {
                id: "s".into(),
                placeholder: Some("Pick".into()),
                options: vec![
                    SelectOption {
                        label: "One".into(),
                        value: "1".into(),
                        default: true,
                    },
                    SelectOption {
                        label: "Two".into(),
                        value: "2".into(),
                        default: false,
                    },
                ],
                min: Some(1),
                max: Some(2),
            }),
            Row::Select(Select {
                id: "t".into(),
                placeholder: None,
                options: vec![],
                min: None,
                max: None,
            }),
        ];
        let old = vec![
            CreateComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(vec![
                CreateButton::new("a")
                    .emoji('👍')
                    .label("L")
                    .style(DiscordStyle::Primary),
                CreateButton::new("b")
                    .emoji('🔁')
                    .style(DiscordStyle::Secondary)
                    .disabled(true),
                CreateButton::new("c")
                    .label("C")
                    .style(DiscordStyle::Success),
                CreateButton::new("d")
                    .style(DiscordStyle::Danger)
                    .disabled(true),
            ]))),
            CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "s",
                    CreateSelectMenuKind::String {
                        options: Cow::Owned(vec![
                            CreateSelectMenuOption::new("One", "1").default_selection(true),
                            CreateSelectMenuOption::new("Two", "2"),
                        ]),
                    },
                )
                .placeholder("Pick")
                .min_values(1)
                .max_values(2),
            )),
            CreateComponent::ActionRow(CreateActionRow::SelectMenu(CreateSelectMenu::new(
                "t",
                CreateSelectMenuKind::String {
                    options: Cow::Owned(vec![]),
                },
            ))),
        ];
        assert_eq!(json(&action_rows(&rows)), json(&old));
    }

    #[test]
    fn a_custom_id_reaches_discord_verbatim() {
        let id = "gp:guess:123456789012345678:3:7";
        let rows = vec![Row::Buttons(vec![btn(
            id,
            None,
            None,
            ButtonStyle::Primary,
            false,
        )])];
        let old = vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
            Cow::Owned(vec![CreateButton::new(id).style(DiscordStyle::Primary)]),
        ))];
        assert_eq!(json(&action_rows(&rows)), json(&old));
        assert!(json(&action_rows(&rows)).contains(id));
    }

    #[test]
    fn mentions_none_allows_nobody() {
        assert_eq!(
            json(&allowed_mentions(Mentions::None)),
            json(&CreateAllowedMentions::new())
        );
    }

    #[test]
    fn mentions_users_allows_users() {
        assert_eq!(
            json(&allowed_mentions(Mentions::Users)),
            json(&CreateAllowedMentions::new().all_users(true))
        );
        assert_ne!(
            json(&allowed_mentions(Mentions::Users)),
            json(&allowed_mentions(Mentions::None))
        );
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
        assert_eq!(content_len(&r.to_message()), CONTENT_MAX);
        assert_eq!(content_len(&r.to_interaction_message()), CONTENT_MAX);
        assert_eq!(content_len(&r.to_followup(false)), CONTENT_MAX);
        assert_eq!(content_len(&r.to_edit()), CONTENT_MAX);
        // 🪤 Waiver: `poise::CreateReply` is not `Serialize` and exposes no
        // content accessor, so `to_reply` and `to_reply_edit` (which share the
        // `content` helper with the four paths above) are not observed here.
    }

    #[test]
    fn an_edit_without_rows_clears_the_buttons() {
        let edit = json(&Rendered::text("x").to_edit());
        assert!(edit.contains("\"components\":[]"), "{edit}");
    }

    #[test]
    fn an_edit_without_content_clears_the_text() {
        let edit = json(&Rendered::default().to_edit());
        assert!(edit.contains("\"content\":\"\""), "{edit}");
    }

    #[test]
    fn mentions_none_survives_every_send_path() {
        let r = Rendered::text("<@123>");
        let none = "\"allowed_mentions\":{\"parse\":[],\"users\":[],\"roles\":[]}";
        for wire in [
            json(&r.to_message()),
            json(&r.to_interaction_message()),
            json(&r.to_followup(false)),
            json(&r.to_edit()),
        ] {
            assert!(wire.contains(none), "{wire}");
        }
    }

    #[test]
    fn the_send_paths_match_the_old_ones() {
        let card = |t: &str| Card {
            title: Some(t.into()),
            ..Card::default()
        };
        let rows = vec![Row::Buttons(vec![btn(
            "a",
            Some("L"),
            None,
            ButtonStyle::Primary,
            false,
        )])];
        let r = Rendered::card(card("one"))
            .with_cards(vec![card("two")])
            .with_content("hello")
            .with_rows(rows);
        let embeds = || {
            vec![
                CreateEmbed::new().title("one"),
                CreateEmbed::new().title("two"),
            ]
        };
        let components = || {
            vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
                Cow::Owned(vec![CreateButton::new("a")
                    .label("L")
                    .style(DiscordStyle::Primary)]),
            ))]
        };
        let mentions = CreateAllowedMentions::new;

        assert_eq!(
            json(&r.to_message()),
            json(
                &CreateMessage::new()
                    .allowed_mentions(mentions())
                    .components(components())
                    .content("hello")
                    .embeds(embeds())
            )
        );
        assert_eq!(
            json(&r.to_interaction_message()),
            json(
                &CreateInteractionResponseMessage::new()
                    .allowed_mentions(mentions())
                    .components(components())
                    .content("hello")
                    .embeds(embeds())
            )
        );
        assert_eq!(
            json(&r.to_followup(true)),
            json(
                &CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .allowed_mentions(mentions())
                    .components(components())
                    .content("hello")
                    .embeds(embeds())
            )
        );
        assert_eq!(
            json(&r.to_edit()),
            json(
                &EditMessage::new()
                    .allowed_mentions(mentions())
                    .components(components())
                    .content("hello")
                    .embeds(embeds())
            )
        );
    }
}
