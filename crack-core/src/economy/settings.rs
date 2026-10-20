//! Per-server economy settings: the on switch plus a typed config stored as
//! JSONB. Missing fields take their defaults, and a price key or a top-level
//! field this build does not know is kept, so a config written by another
//! release survives a load and save.

use serde::{Deserialize, Serialize};
use serenity::all::GuildId;
use std::collections::BTreeMap;

/// Something a member can buy. PR 2 adds variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Immunity,
}

impl ActionKind {
    pub const ALL: &'static [ActionKind] = &[ActionKind::Immunity];

    /// The stable key used in the prices map and the database.
    pub fn key(self) -> &'static str {
        match self {
            ActionKind::Immunity => "immunity",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.key() == key)
    }

    pub fn default_price(self) -> ActionPrice {
        match self {
            ActionKind::Immunity => ActionPrice {
                price: 200,
                on: true,
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Immunity => "Immunity",
        }
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EconomySettingsError as E;

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
            serde_json::from_str(r#"{"max_bet": 50, "rename_hours": 12}"#).unwrap();
        assert_eq!(c.max_bet, 50);
        let raw = serde_json::to_string(&c).unwrap();
        assert!(raw.contains(r#""rename_hours":12"#), "{raw}");
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
        s.config =
            serde_json::from_str(r#"{"prices": {"rename": {"price": 0, "on": true}}}"#).unwrap();
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
}
