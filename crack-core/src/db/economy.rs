//! Economy settings as a row per server, and the opted-out set. See `economy`.

use crate::economy::settings::{EconomyConfig, EconomySettings};
use serenity::all::{GuildId, UserId};
use sqlx::{types::Json, PgPool};
use std::collections::HashSet;

pub async fn load_settings(pool: &PgPool, guild: GuildId) -> sqlx::Result<Option<EconomySettings>> {
    let row = sqlx::query!(
        r#"SELECT enabled, config AS "config: Json<EconomyConfig>"
           FROM economy_settings WHERE guild_id = $1"#,
        guild.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| EconomySettings {
        guild,
        enabled: r.enabled,
        config: r.config.0,
    }))
}

pub async fn save_settings(pool: &PgPool, s: &EconomySettings) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO economy_settings (guild_id, enabled, config)
           VALUES ($1, $2, $3)
           ON CONFLICT (guild_id) DO UPDATE SET
               enabled = EXCLUDED.enabled,
               config = EXCLUDED.config,
               updated_at = now()"#,
        s.guild.get() as i64,
        s.enabled,
        Json(&s.config) as Json<&EconomyConfig>,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The members of a server whose wallet is opted out.
pub async fn load_opted_out(pool: &PgPool, guild: GuildId) -> sqlx::Result<HashSet<UserId>> {
    let rows = sqlx::query!(
        r#"SELECT user_id FROM economy_wallets
           WHERE guild_id = $1 AND opted_out_at IS NOT NULL"#,
        guild.get() as i64
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| u64::try_from(r.user_id).ok().filter(|v| *v != 0))
        .map(UserId::new)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn settings_survive_a_save_and_load(pool: PgPool) -> sqlx::Result<()> {
        let mut s = EconomySettings {
            guild: GuildId::new(1),
            enabled: true,
            config: EconomyConfig::default(),
        };
        s.config.currency_name = "gems".into();
        s.config.max_bet = 42;
        save_settings(&pool, &s).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(s.clone()));
        let off = EconomySettings::off(s.guild);
        save_settings(&pool, &off).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(off), "upsert");
        assert_eq!(load_settings(&pool, GuildId::new(9)).await?, None);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn load_opted_out_reads_only_opted_out_wallets(pool: PgPool) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO economy_wallets (guild_id, user_id, opted_out_at) VALUES
             (1, 10, now()), (1, 11, NULL), (2, 12, now())",
        )
        .execute(&pool)
        .await?;
        assert_eq!(
            load_opted_out(&pool, GuildId::new(1)).await?,
            HashSet::from([UserId::new(10)])
        );
        assert!(load_opted_out(&pool, GuildId::new(3)).await?.is_empty());
        Ok(())
    }
}
