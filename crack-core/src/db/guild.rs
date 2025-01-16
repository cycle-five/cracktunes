use crate::{
    guild::{
        permissions::{GenericPermissionSettings, GenericPermissionSettingsReadWCommand},
        settings::{GuildSettings, WelcomeSettings},
    },
    CrackedResult, Error as SerenityError,
};
use chrono::NaiveDateTime;
use crack_types::errors::CrackedError;
use serde::{Deserialize, Serialize};
use serenity::small_fixed_array::FixedString;
use sqlx::PgPool;
use std::collections::HashMap;

pub struct GuildPermissionPivot {
    pub guild_id: i64,
    pub permission_id: i64,
    pub kind: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GuildSettingsRead {
    pub guild_id: i64,
    pub guild_name: String,
    pub prefix: String,
    pub premium: bool,
    pub autopause: bool,
    pub allow_all_domains: bool,
    pub allowed_domains: Vec<String>,
    pub banned_domains: Vec<String>,
    pub ignored_channels: Vec<i64>,
    pub old_volume: f64,
    pub volume: f64,
    pub self_deafen: bool,
    pub timeout_seconds: Option<i32>,
    pub additional_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WelcomeSettingsRead {
    pub guild_id: i64,
    pub auto_role: Option<i64>,
    pub channel_id: Option<i64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct GuildEntity {
    pub id: i64,
    pub name: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct LogSettingsRead {
    pub guild_id: i64,
    pub all_log_channel: Option<i64>,
    pub raw_event_log_channel: Option<i64>,
    pub server_log_channel: Option<i64>,
    pub member_log_channel: Option<i64>,
    pub join_leave_log_channel: Option<i64>,
    pub voice_log_channel: Option<i64>,
}

impl GuildEntity {
    /// Update the allowed domains for a guild.
    pub async fn write_allowed_domains(
        &self,
        pool: &PgPool,
        allowed_domains: Vec<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE guild_settings
            SET allowed_domains = $2
            WHERE guild_id = $1
            "#,
            self.id,
            &allowed_domains,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update the banned domains for a guild.
    pub async fn write_banned_domains(
        &self,
        pool: &PgPool,
        banned_domains: Vec<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE guild_settings
            set banned_domains = $2
            WHERE guild_id = $1
            "#,
            self.id,
            &banned_domains,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Create or update the log settings for a guild.
    pub async fn write_log_settings(
        pool: &PgPool,
        guild_id: i64,
        settings: &crate::guild::settings::LogSettings,
    ) -> Result<(), crate::CrackedError> {
        sqlx::query!(
            r#"
            INSERT INTO log_settings (guild_id, all_log_channel, raw_event_log_channel, server_log_channel, member_log_channel, join_leave_log_channel, voice_log_channel)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (guild_id)
            DO UPDATE SET all_log_channel = $2, raw_event_log_channel = $3, server_log_channel = $4, member_log_channel = $5, join_leave_log_channel = $6, voice_log_channel = $7
            "#,
            guild_id,
            settings.all_log_channel.map(|x| x as i64),
            settings.raw_event_log_channel.map(|x| x as i64),
            settings.server_log_channel.map(|x| x as i64),
            settings.member_log_channel.map(|x| x as i64),
            settings.join_leave_log_channel.map(|x| x as i64),
            settings.voice_log_channel.map(|x| x as i64),
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Create or update the welcome settings for a guild.
    #[cfg(not(tarpaulin_include))]
    pub async fn write_welcome_settings(
        pool: &PgPool,
        guild_id: i64,
        settings: &crate::guild::settings::WelcomeSettings,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO welcome_settings (guild_id, auto_role, channel_id, message)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (guild_id)
            DO UPDATE SET auto_role = $2, channel_id = $3, message = $4
            "#,
            guild_id,
            settings.auto_role.map(|x| x as i64),
            settings.channel_id.map(|x| x as i64),
            settings.message,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update the premium status for a guild.
    #[cfg(not(tarpaulin_include))]
    pub async fn update_premium(
        pool: &PgPool,
        guild_id: i64,
        premium: bool,
    ) -> Result<GuildSettings, CrackedError> {
        let settings = sqlx::query_as!(
            GuildSettingsRead,
            r#"
            UPDATE guild_settings
            SET premium = $1
            WHERE guild_id = $2
            RETURNING *
            "#,
            premium,
            guild_id,
        )
        .fetch_one(pool)
        .await?;
        Ok(GuildSettings::from(settings))
    }

    /// Write the settings for a guild to the database.
    #[cfg(not(tarpaulin_include))]
    pub async fn write_settings(
        pool: &PgPool,
        settings: &crate::guild::settings::GuildSettings,
    ) -> Result<(), CrackedError> {
        let ignored_channels = settings
            .ignored_channels
            .clone()
            .into_iter()
            .map(|x| x as i64)
            .collect::<Vec<i64>>();
        let to_write = settings.guild_name.to_string();
        sqlx::query!(
            r#"
            INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause, allow_all_domains, allowed_domains, banned_domains, ignored_channels, old_volume, volume, self_deafen, timeout_seconds, additional_prefixes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::FLOAT, $11::FLOAT, $12, $13, $14)
            ON CONFLICT (guild_id)
            DO UPDATE SET guild_name = $2, prefix = $3, premium = $4, autopause = $5, allow_all_domains = $6, allowed_domains = $7, banned_domains = $8, ignored_channels = $9, old_volume = $10::FLOAT, volume = $11::FLOAT, self_deafen = $12, timeout_seconds = $13, additional_prefixes = $14
            "#,
            settings.guild_id.get() as i64,
            to_write,
            settings.prefix,
            settings.premium,
            settings.autopause,
            settings.allow_all_domains,
            &settings.allowed_domains.clone().into_iter().collect::<Vec<String>>(),
            &settings.banned_domains.clone().into_iter().collect::<Vec<String>>(),
            ignored_channels.as_slice(),
            settings.old_volume as i64,
            settings.volume as i64,
            settings.self_deafen,
            settings.timeout as i32,
            &settings.additional_prefixes,
        )
        .execute(pool)
        .await?;

        let guild_id = settings.guild_id.get() as i64;

        let user_perm_arr = &settings
            .authorized_users
            .clone()
            .into_iter()
            .map(|(user, perm)| (user as i64, perm as i64))
            .collect::<Vec<(i64, i64)>>()
            .clone();

        for (user, perm) in user_perm_arr {
            sqlx::query!(
                r#"
                INSERT INTO authorized_users (guild_id, user_id, permissions)
                VALUES ($1, $2, $3)
                ON CONFLICT (guild_id, user_id)
                DO UPDATE SET permissions = $3
                "#,
                guild_id,
                *user,
                *perm,
            )
            .execute(pool)
            .await?;
        }

        let guild_id = settings.guild_id.get();

        if settings.welcome_settings.is_some() {
            settings
                .welcome_settings
                .as_ref()
                .unwrap()
                .save(pool, guild_id)
                .await?;
        }
        if settings.log_settings.is_some() {
            settings
                .log_settings
                .as_ref()
                .unwrap()
                .save(pool, guild_id)
                .await?;
        }

        Ok(())
    }

    /// Get the log settings for a guild from the database.
    pub async fn get_log_settings(
        pool: &PgPool,
        id: i64,
    ) -> Result<Option<crate::guild::settings::LogSettings>, sqlx::Error> {
        let settings_read = sqlx::query_as!(
            LogSettingsRead,
            r#"
            SELECT * FROM log_settings
            WHERE guild_id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?;
        Ok(settings_read.map(crate::guild::settings::LogSettings::from))
    }

    /// Get the welcome settings for a guild from the database.
    pub async fn get_welcome_settings(
        pool: &PgPool,
        id: i64,
    ) -> Result<Option<WelcomeSettings>, sqlx::Error> {
        let settings_read = sqlx::query_as!(
            WelcomeSettingsRead,
            r#"
            SELECT * FROM welcome_settings
            WHERE guild_id = $1
            "#,
            id
        )
        .fetch_optional(pool)
        .await?;
        Ok(settings_read.map(WelcomeSettings::from))
    }

    /// Get the settings for a guild from the database.
    pub async fn get_settings(&self, pool: &PgPool) -> Result<GuildSettings, SerenityError> {
        let settings_opt = sqlx::query_as!(
            GuildSettingsRead,
            r#"
            SELECT * FROM guild_settings
            WHERE guild_id = $1
            "#,
            self.id
        )
        .fetch_optional(pool)
        .await?;
        let settings = match settings_opt {
            Some(settings) => Ok(settings),
            None => {
                sqlx::query_as!(
                    GuildSettingsRead,
                    r#"
                    INSERT INTO guild_settings (guild_id, guild_name)
                    VALUES ($1, $2)
                    ON CONFLICT (guild_id)
                    DO UPDATE SET guild_name = $2
                    RETURNING *
                    "#,
                    self.id,
                    self.name
                )
                .fetch_one(pool)
                .await
            },
        }?;
        let welcome_settings = GuildEntity::get_welcome_settings(pool, self.id).await?;
        let log_settings = GuildEntity::get_log_settings(pool, self.id).await?;
        let cmd_settings = GuildEntity::load_command_settings(self.id, pool).await?;

        Ok(GuildSettings::from(settings)
            .with_welcome_settings(welcome_settings)
            .with_log_settings(log_settings)
            .with_command_settings(cmd_settings))
    }

    /// Create a new guild entity struct, which can be used to interact with the database.
    #[must_use]
    pub fn new_guild(id: i64, name: String) -> GuildEntity {
        GuildEntity {
            id,
            name,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    /// Get a guild entity from the database if it exists.
    pub async fn get(pool: &PgPool, guild_id: i64) -> Result<Option<GuildEntity>, SerenityError> {
        let guild = sqlx::query_as!(
            GuildEntity,
            r#"
            SELECT * FROM guild
            WHERE id = $1
            "#,
            guild_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(guild)
    }

    /// Get a guild entity from the database if it exists, otherwise create it.
    pub async fn get_or_create(
        pool: &PgPool,
        guild_id: i64,
        name: FixedString,
        prefix: String,
    ) -> Result<(GuildEntity, GuildSettings), SerenityError> {
        let name_str = name.to_string();
        let guild_entity = sqlx::query_as!(
            GuildEntity,
            r#"
                INSERT INTO guild (id, name)
                VALUES ($1, $2)
                ON CONFLICT (id)
                DO UPDATE SET name = $2, updated_at = now()
                RETURNING *
                "#,
            guild_id,
            name_str.clone()
        )
        .fetch_one(pool)
        .await?;

        let guild_settings = sqlx::query_as!(
            GuildSettingsRead,
            r#"
                    INSERT INTO guild_settings (guild_id, guild_name, prefix)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (guild_id)
                    DO UPDATE SET guild_name = $2
                    RETURNING *
                    "#,
            guild_id,
            Some(name_str.clone()),
            prefix
        )
        .fetch_one(pool)
        .await?;

        let welcome_settings = GuildEntity::get_welcome_settings(pool, guild_id).await?;
        let log_settings = GuildEntity::get_log_settings(pool, guild_id).await?;
        let command_settings = GuildEntity::load_command_settings(guild_id, pool).await?;
        let guild_settings = GuildSettings::from(guild_settings)
            .with_welcome_settings(welcome_settings)
            .with_log_settings(log_settings)
            .with_command_settings(command_settings);

        Ok((guild_entity, guild_settings))
    }

    /// Update the prefix for the guild.
    pub async fn update_prefix(
        &mut self,
        pool: &PgPool,
        prefix: String,
    ) -> Result<(), SerenityError> {
        self.updated_at = chrono::Utc::now().naive_utc();

        let _ = sqlx::query!(
            r#"
            UPDATE guild_settings
            SET prefix = $1
            WHERE guild_id = $2
            "#,
            prefix,
            self.id,
        )
        .execute(pool)
        .await?;

        sqlx::query!(
            r#"
            UPDATE guild
            SET updated_at = now()
            WHERE id = $1
            "#,
            self.id,
        )
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(std::convert::Into::into)
    }

    /// Load the command settings for a guild.
    pub async fn load_command_settings(
        guild_id: i64,
        pool: &PgPool,
    ) -> CrackedResult<HashMap<String, GenericPermissionSettings>> {
        sqlx::query_as!(
            GenericPermissionSettingsReadWCommand,
            r#"
            SELECT A.command, permission_settings.* FROM
            (SELECT * FROM command_channel WHERE guild_id = $1) as A
            JOIN permission_settings ON A.permission_settings_id = permission_settings.id
            "#,
            guild_id,
        )
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|row| (row.command.clone(), GenericPermissionSettings::from(row)))
                .collect()
        })
        .map_err(std::convert::Into::into)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crack_types::to_fixed;
    use sqlx::PgPool;
    use std::collections::HashSet;
    use std::str::FromStr;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_or_create(pool: PgPool) {
        let (guild, settings) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            to_fixed("test"),
            "test".to_string(),
        )
        .await
        .unwrap();
        assert_eq!(guild.id, 123);
        assert_eq!(guild.name, "test");
        assert_eq!(settings.guild_id.get(), 123);
        assert_eq!(settings.guild_name, "test");
        assert_eq!(settings.prefix, "test");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_update_prefix(pool: PgPool) {
        let (mut guild, _) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            FixedString::from_str("test").unwrap(),
            "test".to_string(),
        )
        .await
        .unwrap();
        guild
            .update_prefix(&pool, "new_prefix".to_string())
            .await
            .unwrap();
        let guild = crate::db::guild::GuildEntity::get(&pool, 123)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(guild.name, "test");
        let settings = guild.get_settings(&pool).await.unwrap();

        assert_eq!(settings.prefix, "new_prefix");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_write_settings(pool: PgPool) {
        let (guild, settings) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            to_fixed("test"),
            "test".to_string(),
        )
        .await
        .unwrap();

        let guild_id = guild.id;

        let welcome_settings = GuildEntity::get_welcome_settings(&pool, guild_id)
            .await
            .unwrap();
        let log_settings = GuildEntity::get_log_settings(&pool, guild_id)
            .await
            .unwrap();

        assert!(welcome_settings.is_none());
        assert!(log_settings.is_none());

        let welcome_settings_new = crate::guild::settings::WelcomeSettings {
            auto_role: Some(123),
            channel_id: Some(123),
            message: Some("test".to_string()),
            password: None,
        };

        let settings = settings.with_welcome_settings(Some(welcome_settings_new.clone()));

        GuildEntity::write_settings(&pool, &settings).await.unwrap();

        let welcome_settings = GuildEntity::get_welcome_settings(&pool, guild_id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(welcome_settings, welcome_settings_new);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_write_banned_comains(pool: PgPool) {
        let (guild, _) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            to_fixed("test"),
            "test".to_string(),
        )
        .await
        .unwrap();

        let banned_domains = vec!["test".to_string(), "test2".to_string()];

        guild
            .write_banned_domains(&pool, banned_domains.clone())
            .await
            .unwrap();

        let settings = guild.get_settings(&pool).await.unwrap();

        assert_eq!(
            settings.banned_domains.iter().collect::<HashSet<_>>(),
            banned_domains.iter().collect::<HashSet<_>>()
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_write_allowed_domains(pool: PgPool) {
        let (guild, _) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            to_fixed("test"),
            "test".to_string(),
        )
        .await
        .unwrap();

        let allowed_domains = vec!["test".to_string(), "test2".to_string()];

        guild
            .write_allowed_domains(&pool, allowed_domains.clone())
            .await
            .unwrap();

        let settings = guild.get_settings(&pool).await.unwrap();

        assert_eq!(
            settings.allowed_domains.iter().collect::<HashSet<_>>(),
            allowed_domains.iter().collect::<HashSet<_>>()
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_write_log_settings(pool: PgPool) {
        let (guild, _) = crate::db::guild::GuildEntity::get_or_create(
            &pool,
            123,
            to_fixed("test"),
            "test".to_string(),
        )
        .await
        .unwrap();

        let guild_id = guild.id;

        let log_settings = crate::guild::settings::LogSettings {
            all_log_channel: Some(123),
            raw_event_log_channel: Some(123),
            server_log_channel: Some(123),
            member_log_channel: Some(123),
            join_leave_log_channel: Some(123),
            voice_log_channel: Some(123),
        };

        GuildEntity::write_log_settings(&pool, guild_id, &log_settings)
            .await
            .unwrap();

        let settings = guild.get_settings(&pool).await.unwrap();

        assert_eq!(settings.log_settings, Some(log_settings));
    }
}
