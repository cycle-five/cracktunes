//! The rooms settings the dashboard shows and saves. The wire types are typed
//! serde; saving goes through crack-core's one save path.

use crack_core::{
    economy::settings::{ActionKind, ActionPrice, EconomySettings, EconomySettingsError},
    messaging::messages::{
        DASH_ECONOMY_BAD_COOLDOWN, DASH_ECONOMY_BAD_CURRENCY_EMOJI, DASH_ECONOMY_BAD_CURRENCY_NAME,
        DASH_ECONOMY_BAD_IMMUNITY_HOURS, DASH_ECONOMY_BAD_MAX_BET,
        DASH_ECONOMY_BAD_OPTOUT_COOLDOWN, DASH_ECONOMY_BAD_PRICE, DASH_ECONOMY_BAD_RATE,
        DASH_ECONOMY_NO_DATABASE, DASH_SETTINGS_BAD_GRACE, DASH_SETTINGS_BAD_LIMIT,
        DASH_SETTINGS_BAD_TEMPLATE, DASH_SETTINGS_NO_DATABASE, DASH_SETTINGS_NO_HUB,
        DASH_SETTINGS_UNKNOWN_CHANNEL, MODLOG_NO_CHANNEL, MODLOG_NO_DATABASE,
    },
    modlog::settings::ModLogSettingsError,
    rooms::settings::SettingsError,
};
use serde::{Deserialize, Serialize};

/// A voice channel the hub can be.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelOption {
    pub id: String,
    pub name: String,
}

/// What the settings page shows. Ids are strings: snowflakes exceed what a
/// JavaScript number holds exactly.
#[derive(Debug, Clone, Serialize)]
pub struct RoomSettingsView {
    pub enabled: bool,
    pub hub: Option<String>,
    pub name_template: String,
    pub default_limit: u8,
    pub empty_grace_secs: u16,
    pub voice_channels: Vec<ChannelOption>,
    /// Rendered permission findings; `None` when they could not be checked
    /// (or there is no hub yet).
    pub findings: Option<Vec<String>>,
}

/// What the page posts.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RoomSettingsForm {
    pub enabled: bool,
    pub hub: Option<String>,
    pub name_template: String,
    pub default_limit: u8,
    pub empty_grace_secs: u16,
}

/// Why a save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveRefused {
    Invalid(SettingsError),
    /// The hub is not a voice channel of this server.
    UnknownChannel,
    /// The database refused the write.
    Failed,
    /// The server's stored settings have not been read yet, so a save
    /// would write defaults over them.
    NotLoaded,
}

/// The reason shown for a refused settings value.
pub fn invalid_message(e: SettingsError) -> &'static str {
    match e {
        SettingsError::NoDatabase => DASH_SETTINGS_NO_DATABASE,
        SettingsError::NoHub => DASH_SETTINGS_NO_HUB,
        SettingsError::BadTemplate => DASH_SETTINGS_BAD_TEMPLATE,
        SettingsError::BadLimit => DASH_SETTINGS_BAD_LIMIT,
        SettingsError::BadGrace => DASH_SETTINGS_BAD_GRACE,
    }
}

/// What the page shows for the mod log. Ids are strings, as for rooms.
#[derive(Debug, Clone, Serialize)]
pub struct ModLogSettingsView {
    pub enabled: bool,
    pub channel: Option<String>,
    pub text_channels: Vec<ChannelOption>,
    /// Rendered permission findings; `None` when they could not be checked
    /// (or there is no channel yet).
    pub findings: Option<Vec<String>>,
    /// "Anyone who can see #channel can read the log."; `None` without a
    /// channel the cache knows.
    pub visibility: Option<String>,
}

/// What the page posts for the mod log.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModLogSettingsForm {
    pub enabled: bool,
    pub channel: Option<String>,
}

/// Why a mod log save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLogSaveRefused {
    Invalid(ModLogSettingsError),
    /// The channel is not a text channel of this server.
    UnknownChannel,
    /// The database refused the write.
    Failed,
    /// The stored settings have not been read yet.
    NotLoaded,
}

/// The reason shown for a refused mod log value.
pub fn modlog_invalid_message(e: ModLogSettingsError) -> &'static str {
    match e {
        ModLogSettingsError::NoDatabase => MODLOG_NO_DATABASE,
        ModLogSettingsError::NoChannel => MODLOG_NO_CHANNEL,
    }
}

pub const UNKNOWN_CHANNEL: &str = DASH_SETTINGS_UNKNOWN_CHANNEL;

/// One price row the page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceView {
    pub kind: String,
    pub label: String,
    pub price: u32,
    pub on: bool,
}

/// What the page shows for the economy: every config field, the prices in
/// `ActionKind::ALL` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EconomySettingsView {
    pub enabled: bool,
    pub currency_name: String,
    pub currency_emoji: String,
    pub voice_credits_per_minute: u32,
    pub message_credits: u32,
    pub message_cooldown_seconds: u32,
    pub prices: Vec<PriceView>,
    pub immunity_optout: bool,
    pub immunity_shop: bool,
    pub immunity_mods: bool,
    pub optout_cooldown_days: u32,
    pub bought_immunity_hours: u32,
    pub gambling_enabled: bool,
    pub max_bet: u32,
}

impl EconomySettingsView {
    pub fn from_settings(s: &EconomySettings) -> Self {
        let c = &s.config;
        Self {
            enabled: s.enabled,
            currency_name: c.currency_name.clone(),
            currency_emoji: c.currency_emoji.clone(),
            voice_credits_per_minute: c.voice_credits_per_minute,
            message_credits: c.message_credits,
            message_cooldown_seconds: c.message_cooldown_seconds,
            prices: ActionKind::ALL
                .iter()
                .map(|k| {
                    let p = c.prices.get(*k);
                    PriceView {
                        kind: k.key().to_owned(),
                        label: k.label().to_owned(),
                        price: p.price,
                        on: p.on,
                    }
                })
                .collect(),
            immunity_optout: c.immunity_optout,
            immunity_shop: c.immunity_shop,
            immunity_mods: c.immunity_mods,
            optout_cooldown_days: c.optout_cooldown_days,
            bought_immunity_hours: c.bought_immunity_hours,
            gambling_enabled: c.gambling_enabled,
            max_bet: c.max_bet,
        }
    }
}

/// One price row the page posts.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PriceForm {
    pub kind: String,
    pub price: u32,
    pub on: bool,
}

/// What the page posts for the economy.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EconomySettingsForm {
    pub enabled: bool,
    pub currency_name: String,
    pub currency_emoji: String,
    pub voice_credits_per_minute: u32,
    pub message_credits: u32,
    pub message_cooldown_seconds: u32,
    pub prices: Vec<PriceForm>,
    pub immunity_optout: bool,
    pub immunity_shop: bool,
    pub immunity_mods: bool,
    pub optout_cooldown_days: u32,
    pub bought_immunity_hours: u32,
    pub gambling_enabled: bool,
    pub max_bet: u32,
}

/// Why an economy save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomySaveRefused {
    Invalid(EconomySettingsError),
    /// A price row names an action this build does not know.
    UnknownAction,
    /// The database refused the write.
    Failed,
    /// The stored settings have not been read yet.
    NotLoaded,
}

/// The reason shown for a refused economy value.
pub fn economy_invalid_message(e: EconomySettingsError) -> &'static str {
    match e {
        EconomySettingsError::NoDatabase => DASH_ECONOMY_NO_DATABASE,
        EconomySettingsError::BadCurrencyName => DASH_ECONOMY_BAD_CURRENCY_NAME,
        EconomySettingsError::BadCurrencyEmoji => DASH_ECONOMY_BAD_CURRENCY_EMOJI,
        EconomySettingsError::BadRate => DASH_ECONOMY_BAD_RATE,
        EconomySettingsError::BadCooldown => DASH_ECONOMY_BAD_COOLDOWN,
        EconomySettingsError::BadPrice => DASH_ECONOMY_BAD_PRICE,
        EconomySettingsError::BadOptoutCooldown => DASH_ECONOMY_BAD_OPTOUT_COOLDOWN,
        EconomySettingsError::BadImmunityHours => DASH_ECONOMY_BAD_IMMUNITY_HOURS,
        EconomySettingsError::BadMaxBet => DASH_ECONOMY_BAD_MAX_BET,
    }
}

/// Merge a posted form into the stored settings. It starts from the stored
/// config and sets each price row, never rebuilding `Prices`, so a price key
/// this build does not know survives the save.
pub fn apply_form(
    stored: EconomySettings,
    form: EconomySettingsForm,
) -> Result<EconomySettings, EconomySaveRefused> {
    let mut s = stored;
    // Resolve every row first so a bad one changes nothing.
    let rows = form
        .prices
        .iter()
        .map(|r| {
            ActionKind::from_key(&r.kind)
                .map(|k| {
                    (
                        k,
                        ActionPrice {
                            price: r.price,
                            on: r.on,
                        },
                    )
                })
                .ok_or(EconomySaveRefused::UnknownAction)
        })
        .collect::<Result<Vec<_>, _>>()?;
    s.enabled = form.enabled;
    let c = &mut s.config;
    c.currency_name = form.currency_name;
    c.currency_emoji = form.currency_emoji;
    c.voice_credits_per_minute = form.voice_credits_per_minute;
    c.message_credits = form.message_credits;
    c.message_cooldown_seconds = form.message_cooldown_seconds;
    for (k, p) in rows {
        c.prices.set(k, p);
    }
    c.immunity_optout = form.immunity_optout;
    c.immunity_shop = form.immunity_shop;
    c.immunity_mods = form.immunity_mods;
    c.optout_cooldown_days = form.optout_cooldown_days;
    c.bought_immunity_hours = form.bought_immunity_hours;
    c.gambling_enabled = form.gambling_enabled;
    c.max_bet = form.max_bet;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_core::economy::settings::EconomyConfig;
    use serenity::all::GuildId;

    fn form() -> EconomySettingsForm {
        EconomySettingsForm {
            enabled: true,
            currency_name: "gems".into(),
            currency_emoji: "X".into(),
            voice_credits_per_minute: 3,
            message_credits: 4,
            message_cooldown_seconds: 90,
            prices: vec![PriceForm {
                kind: "immunity".into(),
                price: 77,
                on: false,
            }],
            immunity_optout: false,
            immunity_shop: false,
            immunity_mods: false,
            optout_cooldown_days: 2,
            bought_immunity_hours: 5,
            gambling_enabled: true,
            max_bet: 9,
        }
    }

    #[test]
    fn saving_keeps_unknown_stored_price_keys() {
        // A config written by a newer release, with a price this build
        // does not know.
        let config: EconomyConfig = serde_json::from_str(
            r#"{"prices":{"future":{"price":5,"on":true},"immunity":{"price":1,"on":true}}}"#,
        )
        .unwrap();
        let stored = EconomySettings {
            guild: GuildId::new(5),
            enabled: false,
            config,
        };
        let out = apply_form(stored, form()).unwrap();
        let raw = serde_json::to_string(&out.config).unwrap();
        assert!(raw.contains(r#""future":{"price":5,"on":true}"#), "{raw}");
        assert_eq!(
            out.config.prices.get(ActionKind::Immunity),
            ActionPrice {
                price: 77,
                on: false
            }
        );
        assert_eq!(out.guild, GuildId::new(5));
    }

    #[test]
    fn apply_form_sets_every_field() {
        let out = apply_form(EconomySettings::off(GuildId::new(5)), form()).unwrap();
        assert!(out.enabled);
        let c = &out.config;
        assert_eq!(c.currency_name, "gems");
        assert_eq!(c.currency_emoji, "X");
        assert_eq!(c.voice_credits_per_minute, 3);
        assert_eq!(c.message_credits, 4);
        assert_eq!(c.message_cooldown_seconds, 90);
        assert!(!c.immunity_optout && !c.immunity_shop && !c.immunity_mods);
        assert_eq!(c.optout_cooldown_days, 2);
        assert_eq!(c.bought_immunity_hours, 5);
        assert!(c.gambling_enabled);
        assert_eq!(c.max_bet, 9);
    }

    #[test]
    fn an_unknown_action_is_refused() {
        let mut f = form();
        f.prices.push(PriceForm {
            kind: "nope".into(),
            price: 1,
            on: true,
        });
        assert_eq!(
            apply_form(EconomySettings::off(GuildId::new(5)), f),
            Err(EconomySaveRefused::UnknownAction)
        );
    }

    #[test]
    fn the_view_lists_prices_in_action_order() {
        let v = EconomySettingsView::from_settings(&EconomySettings::off(GuildId::new(5)));
        assert_eq!(
            v.prices,
            vec![PriceView {
                kind: "immunity".into(),
                label: "Immunity".into(),
                price: 200,
                on: true
            }]
        );
    }
}
