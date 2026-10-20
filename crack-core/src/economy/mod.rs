//! Per-server credits: earned in voice and by chatting, spent in the shop.
//! Off by default; see `economy::settings`.

pub mod discord;
pub mod earn;
pub mod gamble;
pub mod immunity;
pub mod preflight;
pub mod registry;
pub mod settings;
pub mod shop;
pub mod target;
pub mod wallet;

use crate::Data;
use chrono::{DateTime, Utc};
use immunity::{can_switch_optout, is_protected, Protection};
use serenity::all::{GuildId, UserId};
use settings::{EconomySettings, EconomySettingsError};

/// Why [`save_settings`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomySaveError {
    /// The settings failed validation.
    Invalid(EconomySettingsError),
    /// The server's stored settings have not been read yet: a save would
    /// write defaults over them.
    NotLoaded,
    /// The database refused the write.
    Db,
}

/// Read a server's stored settings and opted-out members into the registry
/// and record that they were read (absent counts). Without a database there
/// is nothing to read. False when a database read failed.
pub async fn load_settings(data: &Data, g: GuildId) -> bool {
    let Some(pool) = data.database_pool.as_ref() else {
        return true;
    };
    let found = match crate::db::economy::load_settings(pool, g).await {
        Ok(found) => found,
        Err(err) => {
            tracing::warn!("economy: reading settings of {g} failed: {err}");
            return false;
        },
    };
    let opted_out = match crate::db::economy::load_opted_out(pool, g).await {
        Ok(set) => set,
        Err(err) => {
            tracing::warn!("economy: reading opted-out members of {g} failed: {err}");
            return false;
        },
    };
    // No stored row means off: a repeated GuildCreate must reset stale memory.
    data.economy
        .set_settings(found.unwrap_or_else(|| EconomySettings::off(g)));
    data.economy.set_opted_out(g, opted_out);
    data.economy.mark_loaded(g);
    true
}

/// The one way a server's economy settings change. Validates, writes the
/// database (when there is one), then the registry, so memory never runs
/// ahead of what a restart would load.
pub async fn save_settings(data: &Data, mut s: EconomySettings) -> Result<(), EconomySaveError> {
    if data.database_pool.is_some() && !data.economy.loaded(s.guild) {
        return Err(EconomySaveError::NotLoaded);
    }
    s.validate(data.database_pool.is_some())
        .map_err(EconomySaveError::Invalid)?;
    // Validation judged the trimmed strings; store those.
    s.config.currency_name = s.config.currency_name.trim().to_string();
    s.config.currency_emoji = s.config.currency_emoji.trim().to_string();
    if let Some(pool) = &data.database_pool {
        if let Err(err) = crate::db::economy::save_settings(pool, &s).await {
            tracing::warn!("economy: saving settings of {} failed: {err}", s.guild);
            return Err(EconomySaveError::Db);
        }
    }
    data.economy.set_settings(s);
    Ok(())
}

/// Why [`opt_out`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptOutError {
    /// The server turned member opt-out off (opting back in is still allowed).
    Disabled,
    /// Switched too recently; carries when switching is next allowed.
    Cooldown(DateTime<Utc>),
    /// The member is already in the state asked for.
    AlreadyThere,
    NoDatabase,
    Db,
}

/// Opt a member out of (or back into) the server's economy and pranks.
///
/// Callers must gate on `EconomyRegistry::is_enabled` first: an unloaded
/// server reads default settings here.
pub async fn opt_out(
    data: &Data,
    g: GuildId,
    u: UserId,
    out: bool,
    now: DateTime<Utc>,
) -> Result<(), OptOutError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(OptOutError::NoDatabase);
    };
    let cfg = data.economy.settings(g).config;
    if out && !cfg.immunity_optout {
        return Err(OptOutError::Disabled);
    }
    let state = crate::db::economy::optout_state(pool, g, u)
        .await
        .map_err(|err| {
            tracing::warn!("economy: reading opt-out of {u} in {g} failed: {err}");
            OptOutError::Db
        })?;
    if state.out == out {
        return Err(OptOutError::AlreadyThere);
    }
    can_switch_optout(state.changed_at, now, cfg.optout_cooldown_days)
        .map_err(OptOutError::Cooldown)?;
    crate::db::economy::set_opted_out(pool, g, u, out, now)
        .await
        .map_err(|err| {
            tracing::warn!("economy: writing opt-out of {u} in {g} failed: {err}");
            OptOutError::Db
        })?;
    data.economy.set_one_opted_out(g, u, out);
    if out {
        // PR 2: end running pranks on the target here
    }
    Ok(())
}

/// Whether a member is protected now, from the stored flag and grants.
pub async fn protection(
    data: &Data,
    g: GuildId,
    u: UserId,
    now: DateTime<Utc>,
) -> Result<Option<Protection>, sqlx::Error> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Ok(None);
    };
    let out = crate::db::economy::optout_state(pool, g, u).await?.out;
    let grants = crate::db::economy::grants(pool, g, u).await?;
    Ok(is_protected(out, &grants, now))
}

/// Why [`mod_grant`] or [`mod_lift`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantError {
    /// The server turned moderator grants off.
    Disabled,
    NoDatabase,
    Db,
}

/// A moderator grants a member immunity (`until: None` lasts until lifted).
pub async fn mod_grant(
    data: &Data,
    g: GuildId,
    target: UserId,
    until: Option<DateTime<Utc>>,
    by: UserId,
    reason: Option<&str>,
) -> Result<(), GrantError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(GrantError::NoDatabase);
    };
    if !data.economy.settings(g).config.immunity_mods {
        return Err(GrantError::Disabled);
    }
    crate::db::economy::grant_mod(pool, g, target, until, by, reason)
        .await
        .map_err(|err| {
            tracing::warn!("economy: granting immunity to {target} in {g} failed: {err}");
            GrantError::Db
        })?;
    // PR 2: end running pranks on the target here
    Ok(())
}

/// Remove a member's moderator grant; true when there was one. Always
/// allowed, whatever the server's switches say.
pub async fn mod_lift(data: &Data, g: GuildId, target: UserId) -> Result<bool, GrantError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(GrantError::NoDatabase);
    };
    crate::db::economy::lift_mod(pool, g, target)
        .await
        .map_err(|err| {
            tracing::warn!("economy: lifting immunity of {target} in {g} failed: {err}");
            GrantError::Db
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataInner;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn on() -> EconomySettings {
        EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        }
    }

    #[tokio::test]
    async fn save_refuses_invalid_and_leaves_registry() {
        let data = Data(Arc::new(DataInner::default()));
        let before = data.economy.settings(G);
        assert_eq!(
            save_settings(&data, on()).await,
            Err(EconomySaveError::Invalid(EconomySettingsError::NoDatabase))
        );
        assert_eq!(data.economy.settings(G), before);
        assert!(!data.economy.is_enabled(G));
    }

    #[tokio::test]
    async fn disabling_without_a_pool_is_allowed() {
        let data = Data(Arc::new(DataInner::default()));
        let mut off = EconomySettings::off(G);
        off.config.max_bet = 77;
        assert_eq!(save_settings(&data, off.clone()).await, Ok(()));
        assert_eq!(data.economy.settings(G), off);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn save_refuses_before_load_with_a_pool(pool: sqlx::PgPool) {
        let mut stored = on();
        stored.config.max_bet = 123;
        crate::db::economy::save_settings(&pool, &stored)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO economy_wallets (guild_id, user_id, opted_out_at) VALUES (1, 10, now())",
        )
        .execute(&pool)
        .await
        .unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert_eq!(
            save_settings(&data, on()).await,
            Err(EconomySaveError::NotLoaded)
        );
        assert_eq!(
            crate::db::economy::load_settings(&pool, G).await.unwrap(),
            Some(stored.clone()),
            "nothing written"
        );
        assert!(!data.economy.loaded(G));
        assert!(load_settings(&data, G).await);
        assert!(data.economy.loaded(G));
        assert!(data.economy.is_opted_out(G, serenity::all::UserId::new(10)));
        assert_eq!(data.economy.settings(G), stored);
        assert!(data.economy.is_enabled(G));
        assert_eq!(save_settings(&data, on()).await, Ok(()));
        assert_eq!(
            crate::db::economy::load_settings(&pool, G).await.unwrap(),
            Some(on())
        );
        assert_eq!(data.economy.settings(G), on());
    }

    const U: UserId = UserId::new(10);
    const MOD: UserId = UserId::new(99);

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    async fn live(pool: &sqlx::PgPool, s: EconomySettings) -> Data {
        crate::db::economy::save_settings(pool, &s).await.unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(load_settings(&data, G).await);
        data
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn save_trims_currency_strings(pool: sqlx::PgPool) {
        let data = live(&pool, on()).await;
        let mut s = on();
        s.config.currency_name = " gems ".into();
        s.config.currency_emoji = " 💎 ".into();
        assert_eq!(save_settings(&data, s).await, Ok(()));
        let held = data.economy.settings(G).config;
        assert_eq!(
            (held.currency_name.as_str(), held.currency_emoji.as_str()),
            ("gems", "💎")
        );
        let stored = crate::db::economy::load_settings(&pool, G)
            .await
            .unwrap()
            .unwrap()
            .config;
        assert_eq!(
            (
                stored.currency_name.as_str(),
                stored.currency_emoji.as_str()
            ),
            ("gems", "💎")
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn load_with_no_row_resets_to_off(pool: sqlx::PgPool) {
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        data.economy.set_settings(on());
        assert!(data.economy.is_enabled(G));
        assert!(load_settings(&data, G).await);
        assert!(!data.economy.is_enabled(G));
        assert_eq!(data.economy.settings(G), EconomySettings::off(G));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn turning_a_source_off_keeps_existing_immunity(pool: sqlx::PgPool) {
        let data = live(&pool, on()).await;
        let now = t0();
        crate::db::economy::extend_bought(&pool, G, U, 24, now)
            .await
            .unwrap();
        crate::db::economy::grant_mod(&pool, G, UserId::new(11), None, MOD, None)
            .await
            .unwrap();
        let mut s = on();
        s.config.immunity_shop = false;
        s.config.immunity_mods = false;
        s.config.immunity_optout = false;
        assert_eq!(save_settings(&data, s).await, Ok(()));
        assert_eq!(
            protection(&data, G, U, now).await.unwrap(),
            Some(Protection::Until(now + chrono::Duration::hours(24)))
        );
        assert_eq!(
            protection(&data, G, UserId::new(11), now).await.unwrap(),
            Some(Protection::Indefinitely)
        );
        assert_eq!(
            protection(&data, G, UserId::new(12), now).await.unwrap(),
            None
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn opt_out_updates_db_and_registry(pool: sqlx::PgPool) {
        let data = live(&pool, on()).await;
        let now = t0();
        assert_eq!(opt_out(&data, G, U, true, now).await, Ok(()));
        assert!(data.economy.is_opted_out(G, U));
        let st = crate::db::economy::optout_state(&pool, G, U).await.unwrap();
        assert_eq!((st.out, st.changed_at), (true, Some(now)));
        assert_eq!(
            protection(&data, G, U, now).await.unwrap(),
            Some(Protection::OptedOut)
        );
        // already out
        assert_eq!(
            opt_out(&data, G, U, true, now).await,
            Err(OptOutError::AlreadyThere)
        );
        // back in within the cooldown is refused, state untouched
        let soon = now + chrono::Duration::days(6);
        assert_eq!(
            opt_out(&data, G, U, false, soon).await,
            Err(OptOutError::Cooldown(now + chrono::Duration::days(7)))
        );
        assert!(data.economy.is_opted_out(G, U));
        // after it, allowed
        let later = now + chrono::Duration::days(7);
        assert_eq!(opt_out(&data, G, U, false, later).await, Ok(()));
        assert!(!data.economy.is_opted_out(G, U));
        let st = crate::db::economy::optout_state(&pool, G, U).await.unwrap();
        assert_eq!((st.out, st.changed_at), (false, Some(later)));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn opting_out_is_refused_when_disabled_but_opting_in_is_not(pool: sqlx::PgPool) {
        let now = t0();
        let data = live(&pool, on()).await;
        assert_eq!(opt_out(&data, G, U, true, now).await, Ok(()));
        // The server then turns opt-out off, with a zero cooldown.
        let mut s = on();
        s.config.immunity_optout = false;
        s.config.optout_cooldown_days = 0;
        assert_eq!(save_settings(&data, s).await, Ok(()));
        // Opting back in still works,
        assert_eq!(opt_out(&data, G, U, false, now).await, Ok(()));
        assert!(!data.economy.is_opted_out(G, U));
        // but opting out again is refused, and nothing is written.
        assert_eq!(
            opt_out(&data, G, U, true, now).await,
            Err(OptOutError::Disabled)
        );
        assert!(!data.economy.is_opted_out(G, U));
        assert!(
            !crate::db::economy::optout_state(&pool, G, U)
                .await
                .unwrap()
                .out
        );
    }

    #[tokio::test]
    async fn without_a_pool_opt_out_and_grants_are_refused_and_protection_is_none() {
        let data = Data(Arc::new(DataInner::default()));
        let now = t0();
        assert_eq!(
            opt_out(&data, G, U, true, now).await,
            Err(OptOutError::NoDatabase)
        );
        assert_eq!(
            mod_grant(&data, G, U, None, MOD, None).await,
            Err(GrantError::NoDatabase)
        );
        assert_eq!(mod_lift(&data, G, U).await, Err(GrantError::NoDatabase));
        assert_eq!(protection(&data, G, U, now).await.unwrap(), None);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn mod_grant_then_lift(pool: sqlx::PgPool) {
        let data = live(&pool, on()).await;
        let now = t0();
        assert_eq!(
            mod_grant(&data, G, U, None, MOD, Some("raid")).await,
            Ok(())
        );
        assert_eq!(
            protection(&data, G, U, now).await.unwrap(),
            Some(Protection::Indefinitely)
        );
        let grants = crate::db::economy::grants(&pool, G, U).await.unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].granted_by, Some(MOD));
        assert_eq!(grants[0].reason.as_deref(), Some("raid"));
        // A bought grant survives the lift.
        crate::db::economy::extend_bought(&pool, G, U, 2, now)
            .await
            .unwrap();
        // Lifting works even with grants switched off.
        let mut s = on();
        s.config.immunity_mods = false;
        assert_eq!(save_settings(&data, s).await, Ok(()));
        assert_eq!(
            mod_grant(&data, G, U, None, MOD, None).await,
            Err(GrantError::Disabled)
        );
        assert_eq!(mod_lift(&data, G, U).await, Ok(true));
        assert_eq!(
            protection(&data, G, U, now).await.unwrap(),
            Some(Protection::Until(now + chrono::Duration::hours(2)))
        );
        assert_eq!(mod_lift(&data, G, U).await, Ok(false));
    }
}
