//! Platform-neutral message output: what the bot wants to say, not how a
//! platform says it. Adapters (the Discord one lives in crack-core) convert.

use std::fmt;

use crate::{RoleRef, RoomRef, UserRef};

/// One outgoing message: text, cards and rows of controls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub content: Option<String>,
    pub cards: Vec<Card>,
    pub rows: Vec<Row>,
    pub mentions: Mentions,
}

impl Card {
    /// A card holding only a description.
    #[must_use]
    pub fn described(text: impl Into<String>) -> Self {
        Self {
            description: Some(text.into()),
            ..Self::default()
        }
    }
}

impl Rendered {
    /// Plain text. Stored as given; a platform adapter applies its own limit.
    #[must_use]
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            ..Self::default()
        }
    }

    /// A single card.
    #[must_use]
    pub fn card(card: Card) -> Self {
        Self {
            cards: vec![card],
            ..Self::default()
        }
    }

    /// Sets the text, replacing any already present.
    #[must_use]
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Sets the rows of controls, replacing any already present.
    #[must_use]
    pub fn with_rows(mut self, rows: Vec<Row>) -> Self {
        self.rows = rows;
        self
    }

    /// Appends `cards` after the cards already present.
    #[must_use]
    pub fn with_cards(mut self, cards: Vec<Card>) -> Self {
        self.cards.extend(cards);
        self
    }
}

/// Colours as 0xRRGGBB, the same values as serenity's `Colour` constants.
pub mod colour {
    pub const RED: u32 = 0xE74C3C;
    pub const GOLD: u32 = 0xF1C40F;
    pub const BLUE: u32 = 0x3498DB;
    pub const BLURPLE: u32 = 0x7289DA;
    pub const FOOYOO: u32 = 0x11CA80;
    pub const DARKER_GREY: u32 = 0x546E7A;
    pub const DARK_GREEN: u32 = 0x1F8B4C;
    pub const DARK_GOLD: u32 = 0xC27C0E;
}

/// A rich block: the neutral shape of an embed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    pub title: Option<String>,
    pub url: Option<String>,
    pub description: Option<String>,
    pub author: Option<Author>,
    pub fields: Vec<Field>,
    pub thumbnail: Option<String>,
    pub footer: Option<Footer>,
    /// Unix seconds.
    pub timestamp: Option<i64>,
    /// 0xRRGGBB.
    pub colour: Option<u32>,
}

/// Who a card is by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub url: Option<String>,
    pub icon_url: Option<String>,
}

/// A titled value inside a card; inline fields may sit side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub inline: bool,
}

/// The small text at the foot of a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    pub text: String,
    pub icon_url: Option<String>,
}

/// A row of controls: buttons, or one select, never both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Buttons(Vec<Button>),
    Select(Select),
}

/// A button, identified by its custom id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: String,
    pub label: Option<String>,
    /// A unicode emoji.
    pub emoji: Option<String>,
    pub style: ButtonStyle,
    pub disabled: bool,
}

/// A menu of string options, identified by its custom id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Select {
    pub id: String,
    pub placeholder: Option<String>,
    pub options: Vec<SelectOption>,
    pub min: Option<u8>,
    pub max: Option<u8>,
}

/// How a button is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Success,
    Danger,
}

/// One choice in a select menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub label: String,
    pub value: String,
    pub default: bool,
}

/// Whom a message may notify.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mentions {
    #[default]
    None,
    /// Users mentioned in the text may be pinged. Unused until the welcome
    /// message moves here.
    Users,
}

/// A mention written into message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mention {
    User(UserRef),
    Room(RoomRef),
    Role(RoleRef),
}

impl fmt::Display for Mention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mention::User(r) => write!(f, "<@{}>", r.raw().as_str()),
            Mention::Room(r) => write!(f, "<#{}>", r.raw().as_str()),
            Mention::Role(r) => write!(f, "<@&{}>", r.raw().as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Platform;

    #[test]
    fn a_mention_prints_discords_markdown() {
        let user = Mention::User(UserRef::new(Platform::Discord, "123").unwrap());
        let room = Mention::Room(RoomRef::new(Platform::Discord, "7").unwrap());
        let role = Mention::Role(RoleRef::new(Platform::Discord, "9").unwrap());
        assert_eq!(user.to_string(), "<@123>");
        assert_eq!(room.to_string(), "<#7>");
        assert_eq!(role.to_string(), "<@&9>");
    }

    #[test]
    fn text_keeps_its_content_unclipped() {
        let r = Rendered::text("x".repeat(3000));
        assert_eq!(r.content.unwrap().len(), 3000);
    }

    #[test]
    fn a_card_then_extra_cards_keep_their_order() {
        let card = |t: &str| Card {
            title: Some(t.to_string()),
            ..Card::default()
        };
        let r = Rendered::card(card("a")).with_cards(vec![card("b"), card("c")]);
        assert_eq!(r.cards, vec![card("a"), card("b"), card("c")]);
    }

    #[test]
    fn a_described_card_has_only_a_description() {
        assert_eq!(
            Card::described("hi"),
            Card {
                description: Some("hi".to_string()),
                ..Card::default()
            }
        );
    }

    #[test]
    fn the_default_mentions_nobody() {
        assert_eq!(Rendered::default().mentions, Mentions::None);
    }

    #[test]
    fn the_builders_set_content_and_rows() {
        let row = Row::Buttons(vec![Button {
            id: "go".to_string(),
            label: Some("Go".to_string()),
            emoji: None,
            style: ButtonStyle::Primary,
            disabled: false,
        }]);
        let r = Rendered::default()
            .with_content("hi")
            .with_rows(vec![row.clone()]);
        assert_eq!(r.content.as_deref(), Some("hi"));
        assert_eq!(r.rows, vec![row]);
    }
}
