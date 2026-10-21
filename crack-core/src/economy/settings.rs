//! Per-server economy settings: the on switch plus a typed config stored as
//! JSONB. Missing fields take their defaults, and a price key or a top-level
//! field this build does not know is kept, so a config written by another
//! release survives a load and save.

use crate::messaging::messages::{
    ECONOMY_CLOWN_SUFFIX, ECONOMY_LABEL_BOUNTY, ECONOMY_LABEL_CLEANSE, ECONOMY_LABEL_CLOWN,
    ECONOMY_LABEL_CROWDFUND, ECONOMY_LABEL_DEAFEN, ECONOMY_LABEL_DEDICATE, ECONOMY_LABEL_DRAG,
    ECONOMY_LABEL_IMMUNITY, ECONOMY_LABEL_JAIL, ECONOMY_LABEL_JUMP, ECONOMY_LABEL_LOCK,
    ECONOMY_LABEL_MIRROR, ECONOMY_LABEL_MUTE, ECONOMY_LABEL_RENAME, ECONOMY_LABEL_REVEAL,
    ECONOMY_LABEL_THEME,
};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GuildId};
use std::collections::BTreeMap;

/// Something a member can buy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Immunity,
    Rename,
    Lock,
    Clown,
    Drag,
    Mute,
    Deafen,
    Jail,
    Mirror,
    Cleanse,
    Reveal,
    Theme,
    Dedicate,
    Jump,
    Bounty,
    /// Never a row's kind: only a switch and the minimum pledge.
    Crowdfund,
}

impl ActionKind {
    pub const ALL: &'static [ActionKind] = &[
        ActionKind::Immunity,
        ActionKind::Rename,
        ActionKind::Lock,
        ActionKind::Clown,
        ActionKind::Drag,
        ActionKind::Mute,
        ActionKind::Deafen,
        ActionKind::Jail,
        ActionKind::Mirror,
        ActionKind::Cleanse,
        ActionKind::Reveal,
        ActionKind::Theme,
        ActionKind::Dedicate,
        ActionKind::Jump,
        ActionKind::Bounty,
        ActionKind::Crowdfund,
    ];

    /// The stable key used in the prices map and the database.
    pub fn key(self) -> &'static str {
        match self {
            ActionKind::Immunity => "immunity",
            ActionKind::Rename => "rename",
            ActionKind::Lock => "lock",
            ActionKind::Clown => "clown",
            ActionKind::Drag => "drag",
            ActionKind::Mute => "mute",
            ActionKind::Deafen => "deafen",
            ActionKind::Jail => "jail",
            ActionKind::Mirror => "mirror",
            ActionKind::Cleanse => "cleanse",
            ActionKind::Reveal => "reveal",
            ActionKind::Theme => "theme",
            ActionKind::Dedicate => "dedicate",
            ActionKind::Jump => "jump",
            ActionKind::Bounty => "bounty",
            ActionKind::Crowdfund => "crowdfund",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.key() == key)
    }

    pub fn default_price(self) -> ActionPrice {
        let price = match self {
            ActionKind::Immunity => 200,
            ActionKind::Rename => 120,
            ActionKind::Lock => 60,
            ActionKind::Clown => 30,
            ActionKind::Drag => 60,
            ActionKind::Mute => 90,
            ActionKind::Deafen => 90,
            ActionKind::Jail => 150,
            ActionKind::Mirror => 150,
            ActionKind::Cleanse => 80,
            ActionKind::Reveal => 100,
            ActionKind::Theme => 100,
            ActionKind::Dedicate => 40,
            ActionKind::Jump => 40,
            // The minimum stake.
            ActionKind::Bounty => 50,
            // The minimum pledge.
            ActionKind::Crowdfund => 10,
        };
        ActionPrice { price, on: true }
    }

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Immunity => ECONOMY_LABEL_IMMUNITY,
            ActionKind::Rename => ECONOMY_LABEL_RENAME,
            ActionKind::Lock => ECONOMY_LABEL_LOCK,
            ActionKind::Clown => ECONOMY_LABEL_CLOWN,
            ActionKind::Drag => ECONOMY_LABEL_DRAG,
            ActionKind::Mute => ECONOMY_LABEL_MUTE,
            ActionKind::Deafen => ECONOMY_LABEL_DEAFEN,
            ActionKind::Jail => ECONOMY_LABEL_JAIL,
            ActionKind::Mirror => ECONOMY_LABEL_MIRROR,
            ActionKind::Cleanse => ECONOMY_LABEL_CLEANSE,
            ActionKind::Reveal => ECONOMY_LABEL_REVEAL,
            ActionKind::Theme => ECONOMY_LABEL_THEME,
            ActionKind::Dedicate => ECONOMY_LABEL_DEDICATE,
            ActionKind::Jump => ECONOMY_LABEL_JUMP,
            ActionKind::Bounty => ECONOMY_LABEL_BOUNTY,
            ActionKind::Crowdfund => ECONOMY_LABEL_CROWDFUND,
        }
    }

    /// The seven pranks (spec 4.1): the kinds aimed at another member.
    pub const PRANKS: &'static [ActionKind] = &[
        ActionKind::Rename,
        ActionKind::Lock,
        ActionKind::Clown,
        ActionKind::Drag,
        ActionKind::Mute,
        ActionKind::Deafen,
        ActionKind::Jail,
    ];

    /// What cleanse, cancel, opt-out and a grant end on a member: the pranks
    /// and a waiting theme.
    pub const RUNNING_ON_TARGET: &'static [ActionKind] = &[
        ActionKind::Rename,
        ActionKind::Lock,
        ActionKind::Clown,
        ActionKind::Drag,
        ActionKind::Mute,
        ActionKind::Deafen,
        ActionKind::Jail,
        ActionKind::Theme,
    ];

    /// What `/shop reveal` can name: the pranks and the aimed kinds.
    pub const REVEALABLE: &'static [ActionKind] = &[
        ActionKind::Rename,
        ActionKind::Lock,
        ActionKind::Clown,
        ActionKind::Drag,
        ActionKind::Mute,
        ActionKind::Deafen,
        ActionKind::Jail,
        ActionKind::Theme,
        ActionKind::Dedicate,
        ActionKind::Bounty,
    ];

    /// The pranks a crowd can fund. Drag is out: its destination is the
    /// buyer's channel at the moment it runs.
    pub const FUNDABLE: &'static [ActionKind] = &[
        ActionKind::Rename,
        ActionKind::Clown,
        ActionKind::Lock,
        ActionKind::Mute,
        ActionKind::Deafen,
        ActionKind::Jail,
    ];

    /// Aimed at a member without being a prank.
    pub fn is_aimed(self) -> bool {
        matches!(
            self,
            ActionKind::Theme | ActionKind::Dedicate | ActionKind::Bounty
        )
    }

    /// Their row's end refunds the buyer.
    pub fn is_escrow(self) -> bool {
        matches!(self, ActionKind::Theme | ActionKind::Bounty)
    }

    /// Whether this is one of [`Self::PRANKS`].
    pub fn is_prank(self) -> bool {
        Self::PRANKS.contains(&self)
    }

    /// True when the effect lasts a while and is undone by the sweeper.
    pub fn is_timed(self) -> bool {
        matches!(
            self,
            ActionKind::Rename
                | ActionKind::Lock
                | ActionKind::Clown
                | ActionKind::Mute
                | ActionKind::Deafen
                | ActionKind::Jail
        )
    }

    /// True when a bought row stays `active` until something ends it: the
    /// timed kinds, and a mirror (which stands until it fires). The other
    /// kinds are over once done.
    pub fn stays_active(self) -> bool {
        self.is_timed() || self == ActionKind::Mirror
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPrice {
    pub price: u32,
    pub on: bool,
}

/// Prices by [`ActionKind::key`]. Keys this build does not know are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Prices(BTreeMap<String, ActionPrice>);

impl Prices {
    /// The stored price, or the kind's default when none is stored.
    pub fn get(&self, kind: ActionKind) -> ActionPrice {
        self.0
            .get(kind.key())
            .copied()
            .unwrap_or_else(|| kind.default_price())
    }

    pub fn set(&mut self, kind: ActionKind, price: ActionPrice) {
        self.0.insert(kind.key().to_string(), price);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EconomyConfig {
    pub currency_name: String,
    pub currency_emoji: String,
    pub voice_credits_per_minute: u32,
    pub message_credits: u32,
    pub message_cooldown_seconds: u32,
    pub prices: Prices,
    pub immunity_optout: bool,
    pub immunity_shop: bool,
    pub immunity_mods: bool,
    pub optout_cooldown_days: u32,
    pub bought_immunity_hours: u32,
    pub gambling_enabled: bool,
    pub max_bet: u32,
    /// Hours a rename, lock or clown lasts.
    pub rename_hours: u32,
    /// Minutes a mute, deafen or jail lasts.
    pub short_effect_minutes: u32,
    pub jail_channel_id: Option<ChannelId>,
    pub announce_channel_id: Option<ChannelId>,
    /// Opaque passthrough of top-level fields a later release owns. This
    /// build neither reads nor validates them; it only keeps them, so a
    /// rollback followed by a save does not reset what that release stored.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            currency_name: "credits".to_string(),
            currency_emoji: "🪙".to_string(),
            voice_credits_per_minute: 1,
            message_credits: 1,
            message_cooldown_seconds: 60,
            prices: Prices::default(),
            immunity_optout: true,
            immunity_shop: true,
            immunity_mods: true,
            optout_cooldown_days: 7,
            bought_immunity_hours: 24,
            gambling_enabled: false,
            max_bet: 500,
            rename_hours: 24,
            short_effect_minutes: 5,
            jail_channel_id: None,
            announce_channel_id: None,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EconomySettings {
    pub guild: GuildId,
    pub enabled: bool,
    pub config: EconomyConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomySettingsError {
    NoDatabase,
    BadCurrencyName,
    BadCurrencyEmoji,
    BadRate,
    BadCooldown,
    BadPrice,
    BadOptoutCooldown,
    BadImmunityHours,
    BadMaxBet,
    BadRenameHours,
    BadShortEffect,
}

impl EconomySettings {
    /// The settings of a server that never configured the economy.
    pub fn off(guild: GuildId) -> Self {
        Self {
            guild,
            enabled: false,
            config: EconomyConfig::default(),
        }
    }

    /// Check settings before saving them. Every bound is checked even when
    /// the economy is off; only enabling needs a database.
    pub fn validate(&self, has_db: bool) -> Result<(), EconomySettingsError> {
        use EconomySettingsError as E;
        let c = &self.config;
        if self.enabled && !has_db {
            return Err(E::NoDatabase);
        }
        let name = c.currency_name.trim();
        if !(1..=24).contains(&name.chars().count()) || name.chars().any(char::is_control) {
            return Err(E::BadCurrencyName);
        }
        let emoji = c.currency_emoji.trim();
        if !(1..=64).contains(&emoji.chars().count())
            || emoji
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            return Err(E::BadCurrencyEmoji);
        }
        if c.voice_credits_per_minute > 100 || c.message_credits > 100 {
            return Err(E::BadRate);
        }
        if !(10..=3600).contains(&c.message_cooldown_seconds) {
            return Err(E::BadCooldown);
        }
        // Only kinds this build knows: an unknown key from a newer release
        // may have wider bounds and must not block every save here.
        if ActionKind::ALL
            .iter()
            .any(|k| !(1..=1_000_000).contains(&c.prices.get(*k).price))
        {
            return Err(E::BadPrice);
        }
        if c.optout_cooldown_days > 90 {
            return Err(E::BadOptoutCooldown);
        }
        if !(1..=720).contains(&c.bought_immunity_hours) {
            return Err(E::BadImmunityHours);
        }
        if !(1..=1_000_000).contains(&c.max_bet) {
            return Err(E::BadMaxBet);
        }
        if !(1..=168).contains(&c.rename_hours) {
            return Err(E::BadRenameHours);
        }
        if !(1..=60).contains(&c.short_effect_minutes) {
            return Err(E::BadShortEffect);
        }
        Ok(())
    }
}

/// Why a prank note or nickname was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteError {
    TooLong,
    Empty,
    Mention,
    Link,
}

/// Why a prank nickname was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NickError {
    TooLong,
    Empty,
    Mention,
    Link,
}

fn has_mention(s: &str) -> bool {
    s.contains("<@") || s.contains("<#") || s.contains("@everyone") || s.contains("@here")
}

fn has_link(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("http://") || l.contains("https://")
}

/// A prank note: 1..=200 chars after trim, no mentions, no links. Returns the
/// trimmed text.
pub fn validate_note(raw: &str) -> Result<String, NoteError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(NoteError::Empty);
    }
    if t.chars().count() > 200 {
        return Err(NoteError::TooLong);
    }
    if has_mention(t) {
        return Err(NoteError::Mention);
    }
    if has_link(t) {
        return Err(NoteError::Link);
    }
    Ok(t.to_string())
}

/// A prank nickname: 1..=32 chars after trim, no mentions, no links. Returns
/// the trimmed text.
pub fn validate_nick(raw: &str) -> Result<String, NickError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(NickError::Empty);
    }
    if t.encode_utf16().count() > 32 {
        return Err(NickError::TooLong);
    }
    if has_mention(t) {
        return Err(NickError::Mention);
    }
    if has_link(t) {
        return Err(NickError::Link);
    }
    Ok(t.to_string())
}

/// `current` plus the clown suffix, cut so the result fits Discord's 32
/// limit. Discord counts UTF-16 code units, so the suffix (" 🤡") costs 3 and
/// `current` keeps at most 29. Counting units never allows more than 32 chars
/// either, and the cut is on chars, never bytes.
pub fn clown_name(current: &str) -> String {
    let room = 32 - ECONOMY_CLOWN_SUFFIX.encode_utf16().count();
    let mut used = 0;
    let mut head = String::new();
    for ch in current.chars() {
        used += ch.len_utf16();
        if used > room {
            break;
        }
        head.push(ch);
    }
    head.push_str(ECONOMY_CLOWN_SUFFIX);
    head
}

#[cfg(test)]
mod tests {
    use super::*;
    use EconomySettingsError as E;

    #[test]
    fn stays_active_is_timed_plus_mirror() {
        use ActionKind::*;
        let active = [Rename, Lock, Clown, Mute, Deafen, Jail, Mirror];
        for &k in ActionKind::ALL {
            assert_eq!(k.stays_active(), active.contains(&k), "{k:?}");
        }
        assert!(!Drag.stays_active() && !Immunity.stays_active());
    }

    fn with(f: impl FnOnce(&mut EconomyConfig)) -> Result<(), EconomySettingsError> {
        let mut s = EconomySettings::off(GuildId::new(1));
        f(&mut s.config);
        s.validate(true)
    }

    #[test]
    fn defaults_match_the_spec() {
        let c = EconomyConfig::default();
        assert_eq!(c.currency_name, "credits");
        assert_eq!(c.currency_emoji, "🪙");
        assert_eq!(c.voice_credits_per_minute, 1);
        assert_eq!(c.message_credits, 1);
        assert_eq!(c.message_cooldown_seconds, 60);
        assert!(c.immunity_optout && c.immunity_shop && c.immunity_mods);
        assert_eq!(c.optout_cooldown_days, 7);
        assert_eq!(c.bought_immunity_hours, 24);
        assert!(!c.gambling_enabled);
        assert_eq!(c.max_bet, 500);
        assert_eq!(
            Prices::default().get(ActionKind::Immunity),
            ActionPrice {
                price: 200,
                on: true
            }
        );
        assert_eq!(ActionKind::from_key("immunity"), Some(ActionKind::Immunity));
        assert_eq!(ActionKind::from_key("nope"), None);
        let s = EconomySettings::off(GuildId::new(1));
        assert!(!s.enabled);
        assert_eq!(s.validate(false), Ok(()));
    }

    #[test]
    fn validate_bounds() {
        assert_eq!(with(|_| {}), Ok(()));
        // currency name
        assert_eq!(with(|c| c.currency_name = "a".repeat(24)), Ok(()));
        assert_eq!(
            with(|c| c.currency_name = "a".repeat(25)),
            Err(E::BadCurrencyName)
        );
        assert_eq!(
            with(|c| c.currency_name = "  ".into()),
            Err(E::BadCurrencyName)
        );
        assert_eq!(with(|c| c.currency_name.clear()), Err(E::BadCurrencyName));
        assert_eq!(
            with(|c| c.currency_name = "a\nb".into()),
            Err(E::BadCurrencyName)
        );
        // currency emoji
        assert_eq!(with(|c| c.currency_emoji = "x".repeat(64)), Ok(()));
        assert_eq!(
            with(|c| c.currency_emoji = "x".repeat(65)),
            Err(E::BadCurrencyEmoji)
        );
        assert_eq!(with(|c| c.currency_emoji.clear()), Err(E::BadCurrencyEmoji));
        assert_eq!(
            with(|c| c.currency_emoji = "a b".into()),
            Err(E::BadCurrencyEmoji)
        );
        assert_eq!(
            with(|c| c.currency_emoji = "a\u{7}".into()),
            Err(E::BadCurrencyEmoji)
        );
        // rates
        assert_eq!(
            with(|c| {
                c.voice_credits_per_minute = 100;
                c.message_credits = 0;
            }),
            Ok(())
        );
        assert_eq!(with(|c| c.voice_credits_per_minute = 101), Err(E::BadRate));
        assert_eq!(with(|c| c.message_credits = 101), Err(E::BadRate));
        // cooldown
        assert_eq!(
            with(|c| c.message_cooldown_seconds = 9),
            Err(E::BadCooldown)
        );
        assert_eq!(with(|c| c.message_cooldown_seconds = 10), Ok(()));
        assert_eq!(with(|c| c.message_cooldown_seconds = 3600), Ok(()));
        assert_eq!(
            with(|c| c.message_cooldown_seconds = 3601),
            Err(E::BadCooldown)
        );
        // prices
        let price = |p: u32| ActionPrice { price: p, on: true };
        assert_eq!(
            with(|c| c.prices.set(ActionKind::Immunity, price(0))),
            Err(E::BadPrice)
        );
        assert_eq!(
            with(|c| c.prices.set(ActionKind::Immunity, price(1))),
            Ok(())
        );
        assert_eq!(
            with(|c| c.prices.set(ActionKind::Immunity, price(1_000_000))),
            Ok(())
        );
        assert_eq!(
            with(|c| c.prices.set(ActionKind::Immunity, price(1_000_001))),
            Err(E::BadPrice)
        );
        // opt-out cooldown
        assert_eq!(with(|c| c.optout_cooldown_days = 0), Ok(()));
        assert_eq!(with(|c| c.optout_cooldown_days = 90), Ok(()));
        assert_eq!(
            with(|c| c.optout_cooldown_days = 91),
            Err(E::BadOptoutCooldown)
        );
        // immunity hours
        assert_eq!(
            with(|c| c.bought_immunity_hours = 0),
            Err(E::BadImmunityHours)
        );
        assert_eq!(with(|c| c.bought_immunity_hours = 1), Ok(()));
        assert_eq!(with(|c| c.bought_immunity_hours = 720), Ok(()));
        assert_eq!(
            with(|c| c.bought_immunity_hours = 721),
            Err(E::BadImmunityHours)
        );
        // max bet
        assert_eq!(with(|c| c.max_bet = 0), Err(E::BadMaxBet));
        assert_eq!(with(|c| c.max_bet = 1), Ok(()));
        assert_eq!(with(|c| c.max_bet = 1_000_000), Ok(()));
        assert_eq!(with(|c| c.max_bet = 1_000_001), Err(E::BadMaxBet));
    }

    #[test]
    fn unknown_top_level_fields_survive_a_round_trip() {
        let c: EconomyConfig =
            serde_json::from_str(r#"{"max_bet": 50, "future_field": 12}"#).unwrap();
        assert_eq!(c.max_bet, 50);
        let raw = serde_json::to_string(&c).unwrap();
        assert!(raw.contains(r#""future_field":12"#), "{raw}");
        // Known fields are not duplicated into the passthrough, and the
        // passthrough does not take part in validation.
        assert_eq!(c.extra.len(), 1);
        assert!(EconomyConfig::default().extra.is_empty());
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config = c;
        assert_eq!(s.validate(true), Ok(()));
    }

    #[test]
    fn validate_ignores_unknown_price_keys() {
        let mut s = EconomySettings::off(GuildId::new(1));
        s.config = serde_json::from_str(r#"{"prices": {"future_kind": {"price": 0, "on": true}}}"#)
            .unwrap();
        assert_eq!(s.validate(true), Ok(()));
        s.config
            .prices
            .set(ActionKind::Immunity, ActionPrice { price: 0, on: true });
        assert_eq!(s.validate(true), Err(E::BadPrice));
    }

    #[test]
    fn validate_refuses_enabling_without_db() {
        let on = EconomySettings {
            enabled: true,
            ..EconomySettings::off(GuildId::new(1))
        };
        assert_eq!(on.validate(false), Err(E::NoDatabase));
        assert_eq!(on.validate(true), Ok(()));
        assert_eq!(
            EconomySettings::off(GuildId::new(1)).validate(false),
            Ok(())
        );
    }

    #[test]
    fn config_with_missing_fields_and_unknown_price_loads_and_keeps_it() {
        let c: EconomyConfig = serde_json::from_str(
            r#"{"max_bet": 50, "prices": {"rename": {"price": 9, "on": false}}}"#,
        )
        .unwrap();
        assert_eq!(c.max_bet, 50);
        let d = EconomyConfig::default();
        assert_eq!(
            EconomyConfig {
                max_bet: d.max_bet,
                prices: Prices::default(),
                ..c.clone()
            },
            d
        );
        assert_eq!(
            c.prices.get(ActionKind::Immunity),
            ActionKind::Immunity.default_price()
        );
        assert!(serde_json::to_string(&c).unwrap().contains("\"rename\""));
    }

    #[test]
    fn prank_defaults_match_the_spec() {
        let want = [
            ("immunity", 200),
            ("rename", 120),
            ("lock", 60),
            ("clown", 30),
            ("drag", 60),
            ("mute", 90),
            ("deafen", 90),
            ("jail", 150),
            ("mirror", 150),
            ("cleanse", 80),
            ("reveal", 100),
        ];
        // PR 3 appends its kinds after these; their own test pins them.
        assert_eq!(&ActionKind::ALL[..want.len()], &ActionKind::ALL[..11]);
        assert_eq!(ActionKind::ALL.len(), want.len() + 5);
        for (k, (key, price)) in ActionKind::ALL.iter().zip(want) {
            assert_eq!(k.key(), key);
            assert_eq!(ActionKind::from_key(key), Some(*k));
            assert_eq!(k.default_price(), ActionPrice { price, on: true });
            assert!(!k.label().is_empty());
        }
        let pranks: Vec<_> = ActionKind::ALL.iter().filter(|k| k.is_prank()).collect();
        assert_eq!(pranks.len(), 7);
        for k in [
            ActionKind::Immunity,
            ActionKind::Mirror,
            ActionKind::Cleanse,
            ActionKind::Reveal,
        ] {
            assert!(!k.is_prank(), "{k:?}");
        }
        let timed: Vec<_> = ActionKind::ALL
            .iter()
            .filter(|k| k.is_timed())
            .map(|k| k.key())
            .collect();
        assert_eq!(timed, ["rename", "lock", "clown", "mute", "deafen", "jail"]);
        let c = EconomyConfig::default();
        assert_eq!((c.rename_hours, c.short_effect_minutes), (24, 5));
        assert_eq!((c.jail_channel_id, c.announce_channel_id), (None, None));
    }

    #[test]
    fn new_fields_default_and_bound() {
        assert_eq!(with(|c| c.rename_hours = 0), Err(E::BadRenameHours));
        assert_eq!(with(|c| c.rename_hours = 1), Ok(()));
        assert_eq!(with(|c| c.rename_hours = 168), Ok(()));
        assert_eq!(with(|c| c.rename_hours = 169), Err(E::BadRenameHours));
        assert_eq!(with(|c| c.short_effect_minutes = 0), Err(E::BadShortEffect));
        assert_eq!(with(|c| c.short_effect_minutes = 1), Ok(()));
        assert_eq!(with(|c| c.short_effect_minutes = 60), Ok(()));
        assert_eq!(
            with(|c| c.short_effect_minutes = 61),
            Err(E::BadShortEffect)
        );
        for k in ActionKind::ALL {
            let p = |n| ActionPrice { price: n, on: true };
            assert_eq!(with(|c| c.prices.set(*k, p(0))), Err(E::BadPrice), "{k:?}");
            assert_eq!(
                with(|c| c.prices.set(*k, p(1_000_001))),
                Err(E::BadPrice),
                "{k:?}"
            );
            assert_eq!(with(|c| c.prices.set(*k, p(1_000_000))), Ok(()), "{k:?}");
        }
    }

    #[test]
    fn a_pr1_config_loads_with_pr2_defaults() {
        let c: EconomyConfig = serde_json::from_str(
            r#"{"currency_name":"gems","currency_emoji":"x","voice_credits_per_minute":2,
            "message_credits":1,"message_cooldown_seconds":60,"prices":{"immunity":{"price":5,"on":true}},
            "immunity_optout":true,"immunity_shop":true,"immunity_mods":true,
            "optout_cooldown_days":7,"bought_immunity_hours":24,"gambling_enabled":false,"max_bet":500}"#,
        )
        .unwrap();
        assert_eq!(c.rename_hours, 24);
        assert_eq!(c.short_effect_minutes, 5);
        assert_eq!(c.jail_channel_id, None);
        assert_eq!(c.announce_channel_id, None);
        assert!(c.extra.is_empty());
        assert_eq!(c.prices.get(ActionKind::Rename).price, 120);
    }

    #[test]
    fn group_defaults_match_the_spec() {
        let all: Vec<_> = ActionKind::ALL.iter().map(|k| k.key()).collect();
        assert_eq!(
            &all[11..],
            ["theme", "dedicate", "jump", "bounty", "crowdfund"]
        );
        assert_eq!(all.len(), 16);
        for (k, key, price) in [
            (ActionKind::Theme, "theme", 100),
            (ActionKind::Dedicate, "dedicate", 40),
            (ActionKind::Jump, "jump", 40),
            (ActionKind::Bounty, "bounty", 50),
            (ActionKind::Crowdfund, "crowdfund", 10),
        ] {
            assert_eq!(k.key(), key);
            assert_eq!(ActionKind::from_key(key), Some(k));
            assert_eq!(k.default_price(), ActionPrice { price, on: true });
            assert!(!k.label().is_empty());
        }
    }

    #[test]
    fn kind_groups() {
        use ActionKind::*;
        assert_eq!(
            ActionKind::RUNNING_ON_TARGET,
            [Rename, Lock, Clown, Drag, Mute, Deafen, Jail, Theme]
        );
        assert_eq!(
            ActionKind::REVEALABLE,
            [Rename, Lock, Clown, Drag, Mute, Deafen, Jail, Theme, Dedicate, Bounty]
        );
        assert_eq!(
            ActionKind::FUNDABLE,
            [Rename, Clown, Lock, Mute, Deafen, Jail]
        );
        for &k in ActionKind::ALL {
            assert_eq!(
                k.is_aimed(),
                [Theme, Dedicate, Bounty].contains(&k),
                "{k:?}"
            );
            assert_eq!(k.is_escrow(), [Theme, Bounty].contains(&k), "{k:?}");
            // Aimed kinds are not pranks.
            assert!(!(k.is_aimed() && k.is_prank()), "{k:?}");
        }
    }

    #[test]
    fn a_pr2_config_loads_with_pr3_defaults() {
        let c: EconomyConfig = serde_json::from_str(
            r#"{"prices":{"rename":{"price":9,"on":true},"reveal":{"price":7,"on":false}},
            "rename_hours":12}"#,
        )
        .unwrap();
        assert_eq!(c.rename_hours, 12);
        assert_eq!(c.prices.get(ActionKind::Reveal).price, 7);
        for (k, price) in [
            (ActionKind::Theme, 100),
            (ActionKind::Dedicate, 40),
            (ActionKind::Jump, 40),
            (ActionKind::Bounty, 50),
            (ActionKind::Crowdfund, 10),
        ] {
            assert_eq!(c.prices.get(k), ActionPrice { price, on: true }, "{k:?}");
        }
        assert!(c.extra.is_empty());
    }

    #[test]
    fn a_pr1_extra_field_becomes_typed() {
        let c: EconomyConfig =
            serde_json::from_str(r#"{"rename_hours": 12, "jail_channel_id": "77"}"#).unwrap();
        assert_eq!(c.rename_hours, 12);
        assert_eq!(c.jail_channel_id, Some(ChannelId::new(77)));
        assert!(c.extra.is_empty(), "{:?}", c.extra);
    }

    #[test]
    fn channel_ids_round_trip_as_strings() {
        let c = EconomyConfig {
            jail_channel_id: Some(ChannelId::new(5)),
            announce_channel_id: Some(ChannelId::new(6)),
            ..EconomyConfig::default()
        };
        let raw = serde_json::to_string(&c).unwrap();
        assert!(raw.contains(r#""jail_channel_id":"5""#), "{raw}");
        assert!(raw.contains(r#""announce_channel_id":"6""#), "{raw}");
        assert_eq!(serde_json::from_str::<EconomyConfig>(&raw).unwrap(), c);
        // A number reads too.
        let n: EconomyConfig = serde_json::from_str(r#"{"jail_channel_id": 9}"#).unwrap();
        assert_eq!(n.jail_channel_id, Some(ChannelId::new(9)));
    }

    #[test]
    fn validate_note_and_nick_edges() {
        assert_eq!(validate_note(&"a".repeat(200)).map(|s| s.len()), Ok(200));
        assert_eq!(validate_note(&"a".repeat(201)), Err(NoteError::TooLong));
        assert_eq!(
            validate_note(&"é".repeat(200)).map(|s| s.chars().count()),
            Ok(200)
        );
        assert_eq!(validate_note("  hi  "), Ok("hi".to_string()));
        assert_eq!(validate_note("   "), Err(NoteError::Empty));
        assert_eq!(validate_note(""), Err(NoteError::Empty));
        for m in ["<@1>", "<#2>", "<@&3>", "@everyone", "@here"] {
            assert_eq!(validate_note(m), Err(NoteError::Mention), "{m}");
            assert_eq!(validate_nick(m), Err(NickError::Mention), "{m}");
        }
        assert_eq!(validate_note("https://x"), Err(NoteError::Link));
        assert_eq!(validate_note("see http://x"), Err(NoteError::Link));
        assert_eq!(validate_nick("HTTPS://x"), Err(NickError::Link));
        assert_eq!(validate_nick(&"a".repeat(32)).map(|s| s.len()), Ok(32));
        assert_eq!(validate_nick(&"a".repeat(33)), Err(NickError::TooLong));
        // Discord counts UTF-16 units: a clown is 2, so 16 fit and 17 do not.
        assert_eq!(
            validate_nick(&"🤡".repeat(16)).map(|s| s.chars().count()),
            Ok(16)
        );
        assert_eq!(validate_nick(&"🤡".repeat(17)), Err(NickError::TooLong));
        assert_eq!(
            validate_nick(&format!(" {} ", "a".repeat(32))).map(|s| s.len()),
            Ok(32)
        );
        assert_eq!(validate_nick("  "), Err(NickError::Empty));
        assert_eq!(validate_nick(" bob "), Ok("bob".to_string()));
    }

    #[test]
    fn clown_name_caps_at_32_chars() {
        let long = "a".repeat(32);
        let out = clown_name(&long);
        assert_eq!(out, format!("{} 🤡", "a".repeat(29)));
        // Discord counts UTF-16 units: the clown is two, so 29 + 3 = 32.
        assert_eq!(out.encode_utf16().count(), 32);
        assert!(out.chars().count() <= 32);
        assert_eq!(clown_name("bob"), "bob 🤡");
        assert_eq!(clown_name(""), " 🤡");
        // Multi-byte input is cut on chars, never bytes.
        let wide = clown_name(&"é".repeat(40));
        assert_eq!(wide.chars().count(), 31);
        // A name of astral chars is cut by units too: 14 clowns = 28 units.
        let astral = clown_name(&"🤡".repeat(20));
        assert_eq!(astral, format!("{} 🤡", "🤡".repeat(14)));
        assert!(astral.encode_utf16().count() <= 32);
        assert!(wide.ends_with(" 🤡"));
        let exactly = clown_name(&"a".repeat(29));
        assert_eq!(exactly, format!("{} 🤡", "a".repeat(29)));
    }
}
