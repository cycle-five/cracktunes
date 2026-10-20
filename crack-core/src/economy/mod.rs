//! Per-server credits: earned in voice and by chatting, spent in the shop.
//! Off by default; see `economy::settings`.

pub mod immunity;
pub mod registry;
pub mod settings;
pub mod wallet;

use crate::Data;
use serenity::all::GuildId;
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
    if let Some(s) = found {
        data.economy.set_settings(s);
    }
    data.economy.set_opted_out(g, opted_out);
    data.economy.mark_loaded(g);
    true
}

/// The one way a server's economy settings change. Validates, writes the
/// database (when there is one), then the registry, so memory never runs
/// ahead of what a restart would load.
pub async fn save_settings(data: &Data, s: EconomySettings) -> Result<(), EconomySaveError> {
    if data.database_pool.is_some() && !data.economy.loaded(s.guild) {
        return Err(EconomySaveError::NotLoaded);
    }
    s.validate(data.database_pool.is_some())
        .map_err(EconomySaveError::Invalid)?;
    if let Some(pool) = &data.database_pool {
        if let Err(err) = crate::db::economy::save_settings(pool, &s).await {
            tracing::warn!("economy: saving settings of {} failed: {err}", s.guild);
            return Err(EconomySaveError::Db);
        }
    }
    data.economy.set_settings(s);
    Ok(())
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
}
