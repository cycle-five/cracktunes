use chrono::NaiveDateTime;
use sqlx::PgPool;

use crate::{
    guild::settings::{GuildSettings, WelcomeSettings},
    Error,
};

// CREATE TABLE guild_settings (
//     guild_id BIGINT NOT NULL,
//     guild_name TEXT NOT NULL,
//     prefix TEXT NOT NULL DEFAULT 'r!',
//     premium BOOLEAN NOT NULL DEFAULT FALSE,
//     autopause BOOLEAN NOT NULL DEFAULT FALSE,
//     allow_all_domains BOOLEAN NOT NULL DEFAULT FALSE,
//     allowed_domains TEXT [] NOT NULL DEFAULT '{}',
//     banned_domains TEXT [] NOT NULL DEFAULT '{}',
//     authorized_users BIGINT [] NOT NULL DEFAULT '{}',
//     ignored_channels BIGINT [] NOT NULL DEFAULT '{}',
//     old_volume FLOAT NOT NULL DEFAULT 1.0,
//     volume FLOAT NOT NULL DEFAULT 1.0,
//     self_deafen BOOLEAN NOT NULL DEFAULT FALSE,
//     timeout_seconds INT,
//     welcome_settings_id BIGINT,
//     log_settings_id BIGINT,
//     PRIMARY KEY (guild_id),
//     CONSTRAINT fk_guild_setting FOREIGN KEY (welcome_settings_id) REFERENCES welcome_settings(id),
//     FOREIGN KEY (log_settings_id) REFERENCES log_settings(id)
// );
pub struct GuildSettingsRead {
    pub guild_id: i64,
    pub guild_name: String,
    pub prefix: String,
    pub premium: bool,
    pub autopause: bool,
    pub allow_all_domains: bool,
    pub allowed_domains: Vec<String>,
    pub banned_domains: Vec<String>,
    pub authorized_users: Vec<i64>,
    pub ignored_channels: Vec<i64>,
    pub old_volume: f64,
    pub volume: f64,
    pub self_deafen: bool,
    pub timeout_seconds: Option<i32>,
    pub additional_prefixes: Vec<String>,
}

pub struct WelcomeSettingsRead {
    pub guild_id: i64,
    pub auto_role: Option<i64>,
    pub channel_id: Option<i64>,
    pub message: Option<String>,
}

pub struct GuildEntity {
    pub id: i64,
    pub name: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
// CREATE TABLE log_settings (
//     guild_id BIGINT PRIMARY KEY,
//     all_log_channel BIGINT,
//     raw_event_log_channel BIGINT,
//     server_log_channel BIGINT,
//     member_log_channel BIGINT,
//     join_leave_log_channel BIGINT,
//     voice_log_channel BIGINT,
//     CONSTRAINT fk_log_settings FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
// );
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
    pub async fn write_log_settings(
        &self,
        pool: &PgPool,
        settings: crate::guild::settings::LogSettings,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO log_settings (guild_id, all_log_channel, raw_event_log_channel, server_log_channel, member_log_channel, join_leave_log_channel, voice_log_channel)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (guild_id)
            DO UPDATE SET all_log_channel = $2, raw_event_log_channel = $3, server_log_channel = $4, member_log_channel = $5, join_leave_log_channel = $6, voice_log_channel = $7
            "#,
            self.id,
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

    pub async fn write_welcome_settings(
        &self,
        pool: &PgPool,
        settings: crate::guild::settings::WelcomeSettings,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO welcome_settings (guild_id, auto_role, channel_id, message)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (guild_id)
            DO UPDATE SET auto_role = $2, channel_id = $3, message = $4
            "#,
            self.id,
            settings.auto_role.map(|x| x as i64),
            settings.channel_id.map(|x| x as i64),
            settings.message,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn write_settings(
        &self,
        pool: &PgPool,
        settings: &crate::guild::settings::GuildSettings,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause, allow_all_domains, allowed_domains, banned_domains, authorized_users, ignored_channels, old_volume, volume, self_deafen, timeout_seconds, additional_prefixes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::FLOAT, $12::FLOAT, $13, $14, $15)
            ON CONFLICT (guild_id)
            DO UPDATE SET guild_name = $2, prefix = $3, premium = $4, autopause = $5, allow_all_domains = $6, allowed_domains = $7, banned_domains = $8, authorized_users = $9, ignored_channels = $10, old_volume = $11::FLOAT, volume = $12::FLOAT, self_deafen = $13, timeout_seconds = $14, additional_prefixes = $15
            "#,
            self.id,
            self.name,
            settings.prefix,
            settings.premium,
            settings.autopause,
            settings.allow_all_domains,
            &settings.allowed_domains.clone().into_iter().collect::<Vec<String>>(),
            &settings.banned_domains.clone().into_iter().collect::<Vec<String>>(),
            &settings.authorized_users.clone().into_iter().map(|x| x as i64).collect::<Vec<i64>>(),
            &settings.ignored_channels.clone().into_iter().map(|x| x as i64).collect::<Vec<i64>>(),
            settings.old_volume as i64,
            settings.volume as i64,
            settings.self_deafen,
            settings.timeout as i32,
            &settings.additional_prefixes,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_log_settings(
        &self,
        pool: &PgPool,
    ) -> Result<Option<crate::guild::settings::LogSettings>, sqlx::Error> {
        let settings_read = sqlx::query_as!(
            LogSettingsRead,
            r#"
            SELECT * FROM log_settings
            WHERE guild_id = $1
            "#,
            self.id
        )
        .fetch_optional(pool)
        .await?;
        Ok(settings_read.map(crate::guild::settings::LogSettings::from))
    }

    pub async fn get_welcome_settings(
        &self,
        pool: &PgPool,
    ) -> Result<Option<WelcomeSettings>, sqlx::Error> {
        let settings_read = sqlx::query_as!(
            WelcomeSettingsRead,
            r#"
            SELECT * FROM welcome_settings
            WHERE guild_id = $1
            "#,
            self.id
        )
        .fetch_optional(pool)
        .await?;
        Ok(settings_read.map(WelcomeSettings::from))
    }
    pub async fn get_settings(&self, pool: &PgPool) -> Result<GuildSettings, Error> {
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
            }
        }?;
        Ok(GuildSettings::from(settings))
    }

    pub fn new_guild(id: i64, name: String) -> GuildEntity {
        GuildEntity {
            id,
            name,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    pub async fn get(pool: &PgPool, guild_id: i64) -> Result<Option<GuildEntity>, Error> {
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

    pub async fn get_or_create(
        pool: &PgPool,
        guild_id: i64,
        name: String,
    ) -> Result<GuildEntity, Error> {
        let guild_opt = match sqlx::query_as!(
            GuildEntity,
            r#"
            SELECT * FROM guild
            WHERE id = $1
            "#,
            guild_id
        )
        .fetch_one(pool)
        .await
        {
            Ok(guild) => Some(guild),
            Err(sqlx::Error::RowNotFound) => None,
            Err(e) => return Err(e.into()),
        };

        let guild = match guild_opt {
            Some(guild) => guild,
            None => {
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
                    name
                )
                .fetch_one(pool)
                .await?;

                let _guild_settings = sqlx::query_as!(
                    GuildSettingsRead,
                    r#"
                    INSERT INTO guild_settings (guild_id, guild_name)
                    VALUES ($1, $2)
                    ON CONFLICT (guild_id)
                    DO UPDATE SET guild_name = $2
                    RETURNING *
                    "#,
                    guild_id,
                    Some(name.clone())
                )
                .fetch_one(pool)
                .await?;

                // let guild_settings = GuildSettings::from(guild_settings);
                guild_entity
            }
        };

        Ok(guild)
    }
}

// #[async_trait]
// pub trait ConnectionPool: Sync + Send {
//     async fn connect(&self, url: &str) -> Result<PgPool, Error>;
//     fn to_pg_pool(&self) -> PgPool;
// }

// #[async_trait]
// impl ConnectionPool for PgPool {
//     async fn connect(&self, url: &str) -> Result<PgPool, Error> {
//         let pool = PgPool::connect(url).await?;
//         Ok(pool)
//     }

//     fn to_pg_pool(&self) -> PgPool {
//         self.clone()
//     }
// }

// #[async_trait]
// // #[cfg_attr(test, automock)]
// pub trait GuildRepository {
//     fn new_guild(id: i64, name: String) -> Guild;
//     async fn get(&self, pool: &dyn ConnectionPool, guild_id: i64) -> Result<Option<Guild>, Error>;
//     async fn get_or_create(
//         &self,
//         pool: &dyn ConnectionPool,
//         guild_id: i64,
//         name: String,
//     ) -> Result<Guild, Error>;
// }

// #[async_trait]
// impl GuildRepository for Guild {
//     fn new_guild(id: i64, name: String) -> Guild {
//         Guild {
//             id,
//             name,
//             created_at: chrono::Utc::now().naive_utc(),
//             updated_at: chrono::Utc::now().naive_utc(),
//         }
//     }

//     async fn get(&self, pool: &impl ConnectionPool, guild_id: i64) -> Result<Option<Guild>, Error> {
//         let pool = pool.to_pg_pool();
//         let guild = sqlx::query_as!(
//             Guild,
//             r#"
//             SELECT * FROM guild
//             WHERE id = $1
//             "#,
//             guild_id
//         )
//         .fetch_optional(&pool)
//         .await?;

//         Ok(guild)
//     }

//     async fn get_or_create(
//         &self,
//         pool: &impl ConnectionPool,
//         guild_id: i64,
//         name: String,
//     ) -> Result<Guild, Error> {
//         let pool = pool.to_pg_pool();
//         let guild = sqlx::query_as!(
//             Guild,
//             r#"
//             INSERT INTO guild (id, name)
//             VALUES ($1, $2)
//             ON CONFLICT (id)
//             DO UPDATE SET name = $2, updated_at = now()
//             RETURNING *
//             "#,
//             guild_id,
//             name
//         )
//         .fetch_one(&pool)
//         .await?;

//         Ok(guild)
//     }
// }
// #[cfg(test)]
// mod tests {
//     // Mock the GuildRepository trait
//     use super::*;
//     use mockall::predicate::*;
//     use mockall::*;

//     mock! {
//         ConnectionPool{}

//         #[async_trait]
//         impl ConnectionPool for ConnectionPool {
//             async fn connect(&self, url: &str) -> Result<PgPool, Error>;
//             fn to_pg_pool(&self) -> PgPool;
//         }
//     }
//     mock! {
//         Guild{}

//         #[async_trait]
//         impl GuildRepository for Guild {
//             fn new_guild(id: i64, name: String) -> Guild;
//             async fn get(&self, pool: &dyn ConnectionPool, guild_id: i64) -> Result<Option<Guild>, Error>;
//             async fn get_or_create(&self, pool: &dyn ConnectionPool, guild_id: i64, name: String) -> Result<Guild, Error>;
//         }
//     }

//     #[tokio::test]
//     async fn test_get_or_create() {
//         // let mock = MockGuild::new(1, "asdf".to_string());
//         let mock_guild = MockGuild::new();
//         // let pool = mock(PgPool::connect("postgres://localhost").await.unwrap());
//         let pool = MockConnectionPool::new();
//         let pool = pool.connect("postgres://localhost").await.unwrap();
//         mock_guild
//             .expect_get_or_create()
//             .with(&pool, eq(1), eq("test2".to_string()))
//             .returning(|_, _, _| Ok(Guild::new_guild(1, "test2".to_string())));

//         let guild = mock_guild
//             .get_or_create(&pool, 1, "test2".to_string())
//             .await
//             .unwrap();
//         assert_eq!(guild.name, "test2");

//         let guild = mock_guild.get(&pool, 1).await.unwrap();
//         assert!(guild.is_some());
//     }
// }
