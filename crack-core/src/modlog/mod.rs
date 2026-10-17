//! The mod log: a per-server channel where the bot posts who server-muted,
//! deafened, disconnected or moved whom, and every room kick or mute.

pub mod attribution;
pub mod audit;
pub mod batch;
pub mod event;
pub mod preflight;
pub mod registry;
pub mod settings;
#[cfg(test)]
pub mod test_support;

use crate::Data;
use serenity::all::GuildId;
use settings::{ModLogSettings, ModLogSettingsError};

/// Why [`save_settings`] changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLogSaveError {
    /// The settings failed validation.
    Invalid(ModLogSettingsError),
    /// The server's stored settings have not been read yet: a save would
    /// write defaults over them.
    NotLoaded,
    /// The database refused the write.
    Db,
}

/// Read a server's stored settings into the registry and record that they
/// were read (absent counts). Without a database there is nothing to read.
/// False when the database read failed.
pub async fn load_settings(data: &Data, g: GuildId) -> bool {
    let Some(pool) = data.database_pool.as_ref() else {
        return true;
    };
    match crate::db::modlog::load_settings(pool, g).await {
        Ok(found) => {
            if let Some(s) = found {
                data.modlog.set_settings(s);
            }
            data.modlog.mark_loaded(g);
            true
        },
        Err(err) => {
            tracing::warn!("modlog: reading settings of {g} failed: {err}");
            false
        },
    }
}

/// The one way a server's mod log settings change. Validates, writes the
/// database (when there is one), then the registry, so memory never runs
/// ahead of what a restart would load.
pub async fn save_settings(data: &Data, s: ModLogSettings) -> Result<(), ModLogSaveError> {
    if data.database_pool.is_some() && !data.modlog.loaded(s.guild) {
        return Err(ModLogSaveError::NotLoaded);
    }
    s.validate(data.database_pool.is_some())
        .map_err(ModLogSaveError::Invalid)?;
    if let Some(pool) = &data.database_pool {
        if let Err(err) = crate::db::modlog::save_settings(pool, &s).await {
            tracing::warn!("modlog: saving settings of {} failed: {err}", s.guild);
            return Err(ModLogSaveError::Db);
        }
    }
    let (guild, enabled) = (s.guild, s.enabled);
    data.modlog.set_settings(s);
    // A save lifts a fetch suspension; turning the log off drops all its
    // state (only enabled servers hold any).
    if enabled {
        data.modlog.clear_suspension(guild);
    } else {
        data.modlog.forget(guild);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataInner;
    use serenity::all::ChannelId;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn on() -> ModLogSettings {
        ModLogSettings {
            enabled: true,
            channel: Some(ChannelId::new(55)),
            ..ModLogSettings::off(G)
        }
    }

    #[tokio::test]
    async fn save_refuses_invalid_and_leaves_registry() {
        let data = Data(Arc::new(DataInner::default()));
        let before = data.modlog.settings(G);
        assert_eq!(
            save_settings(&data, on()).await,
            Err(ModLogSaveError::Invalid(ModLogSettingsError::NoDatabase))
        );
        assert_eq!(data.modlog.settings(G), before);
        assert!(!data.modlog.is_enabled(G));
    }

    #[tokio::test]
    async fn disabling_without_a_pool_is_allowed() {
        let data = Data(Arc::new(DataInner::default()));
        let off = ModLogSettings {
            channel: Some(ChannelId::new(7)),
            ..ModLogSettings::off(G)
        };
        assert_eq!(save_settings(&data, off.clone()).await, Ok(()));
        assert_eq!(data.modlog.settings(G), off);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn save_refuses_before_load_with_a_pool(pool: sqlx::PgPool) {
        let stored = ModLogSettings {
            enabled: true,
            channel: Some(ChannelId::new(9)),
            ..ModLogSettings::off(G)
        };
        crate::db::modlog::save_settings(&pool, &stored)
            .await
            .unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert_eq!(
            save_settings(&data, on()).await,
            Err(ModLogSaveError::NotLoaded)
        );
        assert_eq!(
            crate::db::modlog::load_settings(&pool, G).await.unwrap(),
            Some(stored.clone()),
            "nothing written"
        );
        assert!(load_settings(&data, G).await);
        assert_eq!(data.modlog.settings(G), stored);
        assert!(data.modlog.is_enabled(G));
        assert_eq!(save_settings(&data, on()).await, Ok(()));
        assert_eq!(
            crate::db::modlog::load_settings(&pool, G).await.unwrap(),
            Some(on())
        );
        assert_eq!(data.modlog.settings(G), on());
    }
}
