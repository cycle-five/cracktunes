//! The rooms settings the dashboard shows and saves. The wire types are typed
//! serde; saving goes through crack-core's one save path.

use crack_core::{
    economy::settings::{ActionKind, ActionPrice, EconomySettings, EconomySettingsError},
    messaging::messages::{
        DASH_ECONOMY_BAD_COOLDOWN, DASH_ECONOMY_BAD_CURRENCY_EMOJI, DASH_ECONOMY_BAD_CURRENCY_NAME,
        DASH_ECONOMY_BAD_IMMUNITY_HOURS, DASH_ECONOMY_BAD_MAX_BET,
        DASH_ECONOMY_BAD_OPTOUT_COOLDOWN, DASH_ECONOMY_BAD_PRICE, DASH_ECONOMY_BAD_RATE,
        DASH_ECONOMY_BAD_RENAME_HOURS, DASH_ECONOMY_BAD_SHORT_EFFECT, DASH_ECONOMY_NO_DATABASE,
        DASH_SETTINGS_BAD_GRACE, DASH_SETTINGS_BAD_LIMIT, DASH_SETTINGS_BAD_TEMPLATE,
        DASH_SETTINGS_NO_DATABASE, DASH_SETTINGS_NO_HUB, DASH_SETTINGS_UNKNOWN_CHANNEL,
        MODLOG_NO_CHANNEL, MODLOG_NO_DATABASE,
    },
    modlog::settings::ModLogSettingsError,
    rooms::settings::SettingsError,
};
// The report is shared with `/economy status` and `/shop menu`.
pub use crack_core::economy::preflight::{economy_preflight, ActionPreflight};
use serde::{Deserialize, Serialize};
use serenity::all::ChannelId;

/// A voice channel the hub can be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// What the page says under the row, for prices that need explaining.
    pub hint: Option<String>,
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
    pub rename_hours: u32,
    pub short_effect_minutes: u32,
    /// Ids are strings, as for rooms.
    pub jail_channel: Option<String>,
    pub announce_channel: Option<String>,
    /// The jail picker's options. Without a cache: the stored channel only.
    pub voice_channels: Vec<ChannelOption>,
    /// The announcement picker's options.
    pub text_channels: Vec<ChannelOption>,
    /// One entry per prank, in `ActionKind::PRANKS` order.
    pub preflight: Vec<ActionPreflight>,
    /// What the announcement channel lacks for cards; `None` when no
    /// channel is set or the cache cannot say.
    pub announce_findings: Option<Vec<String>>,
}

/// The kind of channel a picker takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Voice,
    Text,
}

fn stored_option(c: Option<ChannelId>) -> Vec<ChannelOption> {
    c.into_iter()
        .map(|id| ChannelOption {
            id: id.to_string(),
            name: format!("channel {id}"),
        })
        .collect()
}

impl EconomySettingsView {
    pub fn from_settings(s: &EconomySettings) -> Self {
        let c = &s.config;
        // No cache here: the pickers hold the stored channel and the report
        // says it can't tell. The live backend fills both in.
        let (preflight, announce_findings) =
            economy_preflight(c, None, |ch| format!("channel {ch}"));
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
                        hint: k.price_hint().map(str::to_owned),
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
            rename_hours: c.rename_hours,
            short_effect_minutes: c.short_effect_minutes,
            jail_channel: c.jail_channel_id.map(|c| c.to_string()),
            announce_channel: c.announce_channel_id.map(|c| c.to_string()),
            voice_channels: stored_option(c.jail_channel_id),
            text_channels: stored_option(c.announce_channel_id),
            preflight,
            announce_findings,
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
    pub rename_hours: u32,
    pub short_effect_minutes: u32,
    pub jail_channel: Option<String>,
    pub announce_channel: Option<String>,
}

/// Why an economy save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomySaveRefused {
    Invalid(EconomySettingsError),
    /// A price row names an action this build does not know.
    UnknownAction,
    /// The jail channel is not a voice channel, or the announcement channel
    /// not a text channel, of this server.
    UnknownChannel,
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
        EconomySettingsError::BadRenameHours => DASH_ECONOMY_BAD_RENAME_HOURS,
        EconomySettingsError::BadShortEffect => DASH_ECONOMY_BAD_SHORT_EFFECT,
    }
}

/// Merge a posted form into the stored settings. It starts from the stored
/// config and sets each price row, never rebuilding `Prices`, so a price key
/// this build does not know survives the save. `resolve` turns a posted
/// channel id into a channel of that kind in this server, or `None`.
pub fn apply_form(
    stored: EconomySettings,
    form: EconomySettingsForm,
    resolve: impl Fn(ChannelKind, &str) -> Option<ChannelId>,
) -> Result<EconomySettings, EconomySaveRefused> {
    let mut s = stored;
    let pick = |kind, raw: &Option<String>| match raw {
        Some(r) => resolve(kind, r)
            .map(Some)
            .ok_or(EconomySaveRefused::UnknownChannel),
        None => Ok(None),
    };
    let jail = pick(ChannelKind::Voice, &form.jail_channel)?;
    let announce = pick(ChannelKind::Text, &form.announce_channel)?;
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
    c.rename_hours = form.rename_hours;
    c.short_effect_minutes = form.short_effect_minutes;
    c.jail_channel_id = jail;
    c.announce_channel_id = announce;
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
            rename_hours: 48,
            short_effect_minutes: 7,
            jail_channel: Some("111".into()),
            announce_channel: Some("222".into()),
        }
    }

    /// Every id is a channel of the right kind.
    fn any_channel(_: ChannelKind, raw: &str) -> Option<ChannelId> {
        raw.parse().ok().map(ChannelId::new)
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
        let out = apply_form(stored, form(), any_channel).unwrap();
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
    fn saving_keeps_unknown_top_level_fields() {
        let config: EconomyConfig =
            serde_json::from_str(r#"{"future_field":12,"max_bet":3}"#).unwrap();
        let stored = EconomySettings {
            guild: GuildId::new(5),
            enabled: false,
            config,
        };
        let out = apply_form(stored, form(), any_channel).unwrap();
        let raw = serde_json::to_string(&out.config).unwrap();
        assert!(raw.contains(r#""future_field":12"#), "{raw}");
        assert_eq!(out.config.rename_hours, 48, "new fields do not disturb it");
        assert_eq!(
            out.config.max_bet, 9,
            "the form still wins on its own fields"
        );
    }

    #[test]
    fn apply_form_sets_every_field() {
        let out = apply_form(EconomySettings::off(GuildId::new(5)), form(), any_channel).unwrap();
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
    fn economy_save_passes_the_new_fields() {
        let out = apply_form(EconomySettings::off(GuildId::new(5)), form(), any_channel).unwrap();
        let c = &out.config;
        assert_eq!(c.rename_hours, 48);
        assert_eq!(c.short_effect_minutes, 7);
        assert_eq!(c.jail_channel_id, Some(ChannelId::new(111)));
        assert_eq!(c.announce_channel_id, Some(ChannelId::new(222)));
        // Empty selects clear both.
        let mut f = form();
        f.jail_channel = None;
        f.announce_channel = None;
        let mut stored = EconomySettings::off(GuildId::new(5));
        stored.config.jail_channel_id = Some(ChannelId::new(5));
        stored.config.announce_channel_id = Some(ChannelId::new(6));
        let out = apply_form(stored, f, any_channel).unwrap();
        assert_eq!(out.config.jail_channel_id, None);
        assert_eq!(out.config.announce_channel_id, None);
    }

    #[test]
    fn economy_save_refuses_a_foreign_jail_channel() {
        let mut stored = EconomySettings::off(GuildId::new(5));
        stored.config.jail_channel_id = Some(ChannelId::new(7));
        // The jail must be a voice channel and the announcement a text one.
        let right_kind = |kind: ChannelKind, raw: &str| match (kind, raw) {
            (ChannelKind::Voice, "111") => Some(ChannelId::new(111)),
            (ChannelKind::Text, "222") => Some(ChannelId::new(222)),
            _ => None,
        };
        assert!(apply_form(stored.clone(), form(), right_kind).is_ok());
        let mut only_jail = form();
        only_jail.announce_channel = None;
        assert!(apply_form(stored.clone(), only_jail, right_kind).is_ok());
        let mut only_announce = form();
        only_announce.jail_channel = None;
        assert!(apply_form(stored.clone(), only_announce, right_kind).is_ok());
        // The same ids as the wrong kinds are refused.
        let wrong_kind = |kind: ChannelKind, raw: &str| match (kind, raw) {
            (ChannelKind::Text, "111") => Some(ChannelId::new(111)),
            (ChannelKind::Voice, "222") => Some(ChannelId::new(222)),
            _ => None,
        };
        let mut only_jail = form();
        only_jail.announce_channel = None;
        assert_eq!(
            apply_form(stored.clone(), only_jail, wrong_kind),
            Err(EconomySaveRefused::UnknownChannel)
        );
        let mut only_announce = form();
        only_announce.jail_channel = None;
        assert_eq!(
            apply_form(stored.clone(), only_announce, wrong_kind),
            Err(EconomySaveRefused::UnknownChannel)
        );
        // Each channel alone is checked.
        let mut f = form();
        f.jail_channel = None;
        assert_eq!(
            apply_form(stored.clone(), f, |_, _| None),
            Err(EconomySaveRefused::UnknownChannel)
        );
        // A foreign id is refused (no cache means nothing resolves).
        assert_eq!(
            apply_form(stored, form(), |_, _| None),
            Err(EconomySaveRefused::UnknownChannel)
        );
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
            apply_form(EconomySettings::off(GuildId::new(5)), f, any_channel),
            Err(EconomySaveRefused::UnknownAction)
        );
    }

    #[test]
    fn the_view_lists_prices_in_action_order() {
        let v = EconomySettingsView::from_settings(&EconomySettings::off(GuildId::new(5)));
        let got: Vec<_> = v
            .prices
            .iter()
            .map(|p| (p.kind.as_str(), p.price, p.on))
            .collect();
        let want: Vec<_> = ActionKind::ALL
            .iter()
            .map(|k| (k.key(), k.default_price().price, true))
            .collect();
        assert_eq!(got, want);
        assert_eq!(got.len(), 16);
        assert_eq!(got[0], ("immunity", 200, true));
        assert_eq!(got[1], ("rename", 120, true));
        assert_eq!(v.prices[1].label, "Rename");
        let hints: Vec<_> = v
            .prices
            .iter()
            .filter_map(|p| Some((p.kind.as_str(), p.hint.as_deref()?)))
            .collect();
        assert_eq!(
            hints,
            vec![
                ("jump", ActionKind::Jump.price_hint().unwrap()),
                ("bounty", "Minimum stake."),
                ("crowdfund", ActionKind::Crowdfund.price_hint().unwrap()),
            ]
        );
    }

    #[test]
    fn save_round_trips_the_new_prices() {
        let mut f = form();
        f.prices = [
            ("theme", 111, true),
            ("dedicate", 41, false),
            ("jump", 42, true),
            ("bounty", 55, true),
            ("crowdfund", 12, false),
        ]
        .into_iter()
        .map(|(k, price, on)| PriceForm {
            kind: k.into(),
            price,
            on,
        })
        .collect();
        let out = apply_form(EconomySettings::off(GuildId::new(5)), f, any_channel).unwrap();
        let v = EconomySettingsView::from_settings(&out);
        let get = |k: &str| {
            let p = v.prices.iter().find(|p| p.kind == k).unwrap();
            (p.price, p.on)
        };
        assert_eq!(get("theme"), (111, true));
        assert_eq!(get("dedicate"), (41, false));
        assert_eq!(get("jump"), (42, true));
        assert_eq!(get("bounty"), (55, true));
        assert_eq!(get("crowdfund"), (12, false));
        // Kinds the form left out keep their defaults.
        assert_eq!(get("rename"), (120, true));
    }
}
