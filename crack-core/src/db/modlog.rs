//! Mod log settings as a row per server. See `modlog`.

use crate::modlog::settings::ModLogSettings;
use serenity::all::{ChannelId, GuildId};
use sqlx::PgPool;

/// A stored id as a serenity id; `None` for zero or negative, which no
/// snowflake is (`new(0)` would panic).
fn positive(id: i64) -> Option<u64> {
    u64::try_from(id).ok().filter(|v| *v != 0)
}

pub async fn load_settings(pool: &PgPool, guild: GuildId) -> sqlx::Result<Option<ModLogSettings>> {
    let row = sqlx::query!(
        r#"SELECT enabled, channel_id FROM mod_log_settings WHERE guild_id = $1"#,
        guild.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ModLogSettings {
        guild,
        enabled: r.enabled,
        channel: r.channel_id.and_then(positive).map(ChannelId::new),
    }))
}

pub async fn save_settings(pool: &PgPool, s: &ModLogSettings) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO mod_log_settings (guild_id, enabled, channel_id)
           VALUES ($1, $2, $3)
           ON CONFLICT (guild_id) DO UPDATE SET
               enabled = EXCLUDED.enabled,
               channel_id = EXCLUDED.channel_id,
               updated_at = now()"#,
        s.guild.get() as i64,
        s.enabled,
        s.channel.map(|c| c.get() as i64),
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn settings_survive_a_save_and_load(pool: PgPool) -> sqlx::Result<()> {
        let s = ModLogSettings {
            guild: GuildId::new(1),
            enabled: true,
            channel: Some(ChannelId::new(55)),
        };
        save_settings(&pool, &s).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(s.clone()));
        let off = ModLogSettings::off(s.guild);
        save_settings(&pool, &off).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(off), "upsert");
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn missing_settings_load_as_none(pool: PgPool) -> sqlx::Result<()> {
        assert_eq!(load_settings(&pool, GuildId::new(9)).await?, None);
        Ok(())
    }
}
