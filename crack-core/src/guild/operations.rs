use crate::{errors::CrackedError, GuildSettings};
use crack_types::discord::{from_snowflake, generic_channel_id, snowflake, ToRef};
use serenity::all::{Context as SerenityContext, GenericChannelId, GuildId};
use std::{collections::HashMap, future::Future, sync::Arc};
use tokio::sync::RwLock;

use super::settings::{Provenance, DEFAULT_PREFIX, DEFAULT_VOLUME_LEVEL};

pub trait GuildSettingsOperations {
    fn get_guild_settings(&self, guild_id: GuildId) -> impl Future<Output = Option<GuildSettings>>;
    fn set_guild_settings(
        &self,
        guild_id: GuildId,
        settings: GuildSettings,
    ) -> impl Future<Output = Option<GuildSettings>>;
    fn get_or_create_guild_settings(
        &self,
        guild_id: GuildId,
        name: Option<String>,
        prefix: Option<&str>,
    ) -> impl Future<Output = GuildSettings>;
    fn ensure_settings_loaded(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<(), CrackedError>>;
    fn get_music_channel(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Option<GenericChannelId>>;
    fn set_music_channel(
        &self,
        guild_id: GuildId,
        channel_id: GenericChannelId,
    ) -> impl Future<Output = ()>;
    fn get_timeout(&self, guild_id: GuildId) -> impl Future<Output = Option<u32>>;
    fn set_timeout(&self, guild_id: GuildId, timeout: u32) -> impl Future<Output = ()>;
    fn get_premium(&self, guild_id: GuildId) -> impl Future<Output = Option<bool>>;
    fn set_premium(&self, guild_id: GuildId, premium: bool) -> impl Future<Output = ()>;
    fn get_prefix(&self, guild_id: GuildId) -> impl Future<Output = Option<String>>;
    fn set_prefix(&self, guild_id: GuildId, prefix: String) -> impl Future<Output = ()>;
    fn add_prefix(&self, guild_id: GuildId, prefix: String) -> impl Future<Output = ()>;
    fn get_additional_prefixes(&self, guild_id: GuildId) -> impl Future<Output = Vec<String>>;
    fn set_additional_prefixes(
        &self,
        guild_id: GuildId,
        prefixes: Vec<String>,
    ) -> impl Future<Output = ()>;
    fn get_autopause(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn set_autopause(&self, guild_id: GuildId, autopause: bool) -> impl Future<Output = ()>;
    fn toggle_autopause(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn get_auto_role(&self, guild_id: GuildId) -> impl Future<Output = Option<u64>>;
    fn set_auto_role(&self, guild_id: GuildId, auto_role: u64) -> impl Future<Output = ()>;
    fn set_autoplay_setting(&self, guild_id: GuildId, autoplay: bool) -> impl Future<Output = ()>;
    fn get_autoplay_setting(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn get_volume(&self, guild_id: GuildId) -> impl Future<Output = (f32, f32)>;
    /// Set the volume (a fraction, 0.5 = 50%), returning the one it replaced.
    fn set_volume(&self, guild_id: GuildId, volume: f32) -> impl Future<Output = f32>;
    fn get_reply_with_embed(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn set_reply_with_embed(&self, guild_id: GuildId, as_embed: bool)
        -> impl Future<Output = bool>;
    fn get_ephemeral_replies(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn toggle_ephemeral_replies(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<bool, CrackedError>>;
    fn get_control_echoes(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn toggle_control_echoes(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<bool, CrackedError>>;
    fn get_now_playing_buttons(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn toggle_now_playing_buttons(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<bool, CrackedError>>;
}

/// The guild settings' state, as a handle that does not name `Data`.
#[derive(Debug, Clone)]
pub struct GuildData {
    pub database_pool: Option<sqlx::PgPool>,
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>,
    pub bot_prefix: String,
}

impl Default for GuildData {
    fn default() -> Self {
        Self {
            database_pool: None,
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            bot_prefix: DEFAULT_PREFIX.to_string(),
        }
    }
}

/// Implementation of the guild settings operations.
impl GuildSettingsOperations for GuildData {
    /// Get the guild settings for a guild, creating them if they don't exist.
    async fn get_or_create_guild_settings(
        &self,
        guild_id: GuildId,
        name: Option<String>,
        prefix: Option<&str>,
    ) -> GuildSettings {
        // 🪤 Not `unwrap_or({ .. })`: its argument is evaluated before the
        // call, so the defaults were built and stored over the loaded settings
        // on every call, returning the old copy so nothing looked wrong.
        if let Some(settings) = self.get_guild_settings(guild_id).await {
            return settings;
        }
        let settings = GuildSettings::new(guild_id.to_ref(), prefix, name);
        self.set_guild_settings(guild_id, settings.clone()).await;
        settings
    }

    /// Get the guild settings for a guild.
    async fn get_guild_settings(&self, guild_id: GuildId) -> Option<GuildSettings> {
        self.guild_settings_map.read().await.get(&guild_id).cloned()
    }

    /// Set the guild settings for a guild.
    async fn set_guild_settings(
        &self,
        guild_id: GuildId,
        settings: GuildSettings,
    ) -> Option<GuildSettings> {
        self.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings)
    }

    /// Get the music channel for the guild.
    async fn get_music_channel(&self, guild_id: GuildId) -> Option<GenericChannelId> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .and_then(|x| x.get_music_channel())
            .and_then(|room| generic_channel_id(&room).ok())
    }

    /// Set the music channel for the guild.
    async fn set_music_channel(&self, guild_id: GuildId, channel_id: GenericChannelId) {
        let mut guard = self.guild_settings_map.write().await;
        let _ = guard
            .entry(guild_id)
            .and_modify(|x| x.set_music_channel(channel_id.to_ref()))
            .or_insert_with(|| GuildSettings::new(guild_id.to_ref(), None, None));
    }

    /// Make sure this guild's in-memory settings came from Postgres, before a
    /// command mutates them and writes them back.
    ///
    /// 🔑 **Call this before any command path that ends in
    /// [`GuildSettings::save`].** A guild whose boot load failed is holding
    /// `GuildSettings::new()` defaults. `save()` is a full-row 14-column
    /// upsert, so mutating those defaults and saving them replaces the guild's
    /// stored row wholesale -- the same unrecoverable loss the shutdown
    /// handler's `is_persistable()` guard prevents, reached through a
    /// different door. See [`crate::guild::settings::Provenance`].
    ///
    /// 🪤 **The guard belongs here, not inside `save()`.** `save()` itself
    /// calls `get_or_create`, so a blanket `is_persistable()` check in there
    /// would refuse the legitimate first write for a guild that has no row
    /// yet, silently doing nothing.
    ///
    /// Returns `Ok(())` and changes nothing when there is no pool: with no
    /// database there is nothing to overwrite and no write to guard.
    ///
    /// No lock is held across the `get_or_create` await -- both read guards
    /// below are statement-scoped temporaries. There is a benign race: a
    /// concurrent command could mutate the map between the read and the write,
    /// and lose that mutation to the freshly-loaded row. Deliberately not
    /// serialised, because the harm is bounded to an in-memory change on
    /// settings that were `Fallback` and therefore unsafe to persist anyway --
    /// the stored row, which is what this guard protects, is never at risk.
    async fn ensure_settings_loaded(&self, guild_id: GuildId) -> Result<(), CrackedError> {
        let Some(pool) = self.database_pool.as_ref() else {
            return Ok(());
        };

        // Already round-tripped through Postgres -- nothing to do. Read lock is
        // dropped before the await below.
        if self
            .guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_some_and(|s| s.is_persistable())
        {
            return Ok(());
        }

        let name = self
            .guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|s| s.guild_name.clone())
            .unwrap_or_default();
        let prefix = self.bot_prefix.clone();

        // 🪤 Returning Err here is the point: the caller must NOT fall through to
        // `save()`. If the row cannot be read, whatever is in memory is still
        // defaults, and writing those back is the loss this guard exists to
        // stop. The detail goes to the log; the caller only needs "not safe".
        let (_guild, settings) =
            crate::db::GuildEntity::get_or_create(pool, guild_id.get() as i64, name, prefix)
                .await
                .map_err(|err| {
                    tracing::error!(
                        "Refusing to write guild {guild_id} settings: could not re-load \
                         them from the database: {err}"
                    );
                    CrackedError::Other("guild settings could not be loaded; refusing to overwrite")
                })?;

        self.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings.with_provenance(Provenance::Database));

        Ok(())
    }

    /// Get the idle timeout for the bot in VC for the guild.
    async fn get_timeout(&self, guild_id: GuildId) -> Option<u32> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|x| x.timeout)
    }

    /// Set the idle timeout for the bot in VC for the guild.
    async fn set_timeout(&self, guild_id: GuildId, timeout: u32) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.timeout = timeout;
            })
            .key();
    }

    /// Get the premium status for a guild.
    async fn get_premium(&self, guild_id: GuildId) -> Option<bool> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|x| x.premium)
    }

    /// Set the premium status for a guild.
    async fn set_premium(&self, guild_id: GuildId, premium: bool) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.premium = premium;
            });
    }

    /// Get the prefix for a guild.
    async fn get_prefix(&self, guild_id: GuildId) -> Option<String> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|x| x.prefix.clone())
    }

    /// Set the prefix for a guild.
    async fn set_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.prefix = prefix;
            });
    }

    /// Add a prefix to the additional prefixes in guild settings.
    async fn add_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.additional_prefixes.push(prefix);
            });
    }

    /// Get the additional prefixes
    async fn get_additional_prefixes(&self, guild_id: GuildId) -> Vec<String> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|x| x.additional_prefixes.clone())
            .unwrap_or_default()
    }

    /// Add a prefix to the additional prefixes in guild settings.
    async fn set_additional_prefixes(&self, guild_id: GuildId, prefixes: Vec<String>) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.additional_prefixes = prefixes;
            });
    }

    /// Get the current autopause settings.
    async fn get_autopause(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|x| x.autopause)
            .unwrap_or(false)
    }

    /// Set the autopause setting.
    async fn set_autopause(&self, guild_id: GuildId, autopause: bool) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.autopause = autopause;
            });
    }

    /// Toggle the autopause setting.
    async fn toggle_autopause(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.autopause = !e.autopause;
            })
            .or_insert_with(|| GuildSettings::new(guild_id.to_ref(), None, None))
            .autopause
    }

    /// Get the current auto role for the guild.
    async fn get_auto_role(&self, guild_id: GuildId) -> Option<u64> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .and_then(|x| x.welcome_settings.as_ref())
            .and_then(|x| x.auto_role.as_ref())
            .and_then(|role| snowflake(role).ok())
    }

    /// Set the auto role for the guild.
    async fn set_auto_role(&self, guild_id: GuildId, auto_role: u64) {
        let Ok(auto_role) = from_snowflake::<crack_model::Role>(auto_role) else {
            tracing::warn!(auto_role, "ignored an auto role id that is not a snowflake");
            return;
        };
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.set_auto_role(Some(auto_role.clone()));
            });
    }

    async fn set_autoplay_setting(&self, guild_id: GuildId, autoplay: bool) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.autoplay = autoplay;
            });
    }

    async fn get_autoplay_setting(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|e| e.autoplay)
            .unwrap_or(true)
    }

    /// Get the current autoplay settings.
    async fn get_volume(&self, guild_id: GuildId) -> (f32, f32) {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|settings| (settings.volume, settings.old_volume))
            .unwrap_or((DEFAULT_VOLUME_LEVEL, DEFAULT_VOLUME_LEVEL))
    }

    /// Set the volume (a fraction, 0.5 = 50%), returning the one it replaced.
    /// Never creates settings: they exist whenever the bot is in voice, and an
    /// inserted default would clobber what loads later (see v0.19.4).
    async fn set_volume(&self, guild_id: GuildId, vol: f32) -> f32 {
        match self.guild_settings_map.write().await.get_mut(&guild_id) {
            Some(s) => {
                let old = s.volume;
                s.old_volume = old;
                s.volume = vol;
                old
            },
            None => DEFAULT_VOLUME_LEVEL,
        }
    }

    /// Get the current reply with embed setting.
    async fn get_reply_with_embed(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_none_or(|x| x.reply_with_embed)
    }

    /// Set the reply with embed setting.
    async fn set_reply_with_embed(&self, guild_id: GuildId, as_embed: bool) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map_or(as_embed, |x| x.reply_with_embed)
    }

    /// Whether /play, /skip and /nowplaying reply ephemerally in this guild.
    async fn get_ephemeral_replies(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_some_and(|settings| settings.ephemeral_replies)
    }

    /// Flip whether /play, /skip and /nowplaying reply ephemerally, save it, and
    /// return the new value.
    ///
    /// 🔑 The stored row is loaded before mutating: a guild whose boot load
    /// failed holds defaults, and `save()` is a full-row upsert that would write
    /// them over it. Without a database pool the flip is in memory only.
    async fn toggle_ephemeral_replies(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.ensure_settings_loaded(guild_id).await?;
        let settings = self
            .guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|settings| {
                settings.toggle_ephemeral_replies();
            })
            .or_insert_with(|| {
                let mut settings =
                    GuildSettings::new(guild_id.to_ref(), Some(&self.bot_prefix), None);
                settings.toggle_ephemeral_replies();
                settings
            })
            .clone();
        if let Some(pool) = self.database_pool.as_ref() {
            settings.save(pool).await?;
        }
        Ok(settings.ephemeral_replies)
    }

    /// Whether controls (dashboard, buttons) post an echo line. On when the
    /// guild has no settings loaded: echoes were always on before v0.23.0.
    async fn get_control_echoes(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_none_or(|settings| settings.control_echoes)
    }

    /// Flip whether controls echo, save it, and return the new value.
    ///
    /// 🔑 As with `toggle_ephemeral_replies`: load the stored row first, or
    /// the full-row upsert writes defaults over it.
    async fn toggle_control_echoes(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.ensure_settings_loaded(guild_id).await?;
        let settings = self
            .guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|settings| {
                settings.toggle_control_echoes();
            })
            .or_insert_with(|| {
                let mut settings =
                    GuildSettings::new(guild_id.to_ref(), Some(&self.bot_prefix), None);
                settings.toggle_control_echoes();
                settings
            })
            .clone();
        if let Some(pool) = self.database_pool.as_ref() {
            settings.save(pool).await?;
        }
        Ok(settings.control_echoes)
    }

    /// Whether the now-playing message carries its buttons. On when the guild
    /// has no settings loaded: the buttons were always shown before v0.25.0.
    async fn get_now_playing_buttons(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .is_none_or(|settings| settings.now_playing_buttons)
    }

    /// Flip whether the now-playing message carries its buttons, save it, and
    /// return the new value.
    ///
    /// 🔑 As with `toggle_control_echoes`: load the stored row first, or the
    /// full-row upsert writes defaults over it.
    async fn toggle_now_playing_buttons(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.ensure_settings_loaded(guild_id).await?;
        let settings = self
            .guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|settings| {
                settings.toggle_now_playing_buttons();
            })
            .or_insert_with(|| {
                let mut settings =
                    GuildSettings::new(guild_id.to_ref(), Some(&self.bot_prefix), None);
                settings.toggle_now_playing_buttons();
                settings
            })
            .clone();
        if let Some(pool) = self.database_pool.as_ref() {
            settings.save(pool).await?;
        }
        Ok(settings.now_playing_buttons)
    }
}

/// Get all guilds the bot is in (that are cached).
pub async fn get_guilds(ctx: Arc<SerenityContext>) -> Vec<GuildId> {
    ctx.http
        .get_guilds(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod test {
    use super::*;
    use serenity::model::id::{GenericChannelId, RoleId};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_get_guild_settings() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None),
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(
            data.get_guild_settings(guild_id).await,
            Some(crate::GuildSettings::new(
                guild_id.to_ref(),
                Some("r!"),
                None
            ))
        );
    }

    #[tokio::test]
    async fn test_get_or_create_guild_settings() {
        let guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(
            data.get_or_create_guild_settings(guild_id, None, None)
                .await,
            crate::GuildSettings {
                guild_id: guild_id.to_ref(),
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            }
        );
    }

    /// 🪤 Settings a guild already has must survive the call untouched. Until
    /// v0.19.4 the default-building block ran on every call (it was the
    /// argument to an eager `unwrap_or`), so every voice join replaced the
    /// guild's loaded settings in memory with defaults: premium off, and marked
    /// not-from-the-database. `/premium grant` then re-loaded the row with the
    /// defaults' empty name and blanked the stored one.
    #[tokio::test]
    async fn get_or_create_keeps_the_settings_a_guild_already_has() {
        use crate::guild::settings::Provenance;
        let guild_id = GuildId::new(1);
        let loaded =
            crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), Some("KYSCORD".to_string()))
                .with_premium(true)
                .with_provenance(Provenance::Database);
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(HashMap::from([(guild_id, loaded.clone())]))),
            ..Default::default()
        };

        let got = data
            .get_or_create_guild_settings(guild_id, None, None)
            .await;
        assert_eq!(got, loaded);

        let kept = data
            .get_guild_settings(guild_id)
            .await
            .expect("still there");
        assert_eq!(kept, loaded);
        assert!(kept.premium);
        assert!(
            kept.is_persistable(),
            "the settings lost their database provenance"
        );
    }

    #[tokio::test]
    async fn test_set_guild_settings() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None),
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_guild_settings(
            guild_id,
            crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None),
        )
        .await;

        assert_eq!(
            data.get_guild_settings(guild_id).await,
            Some(crate::GuildSettings::new(
                guild_id.to_ref(),
                Some("r!"),
                None
            ))
        );
    }

    /// `/ephemeral` flips the saved setting and answers with the new value. A
    /// test has no database pool, so the load-before-write and the save are
    /// skipped; the flip, and the guild it lands on, are what this pins.
    #[tokio::test]
    async fn toggling_ephemeral_replies_flips_it_and_reports_the_new_value() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);

        assert!(data
            .toggle_ephemeral_replies(guild_id)
            .await
            .expect("no pool, nothing to fail"));
        assert!(data.get_ephemeral_replies(guild_id).await);

        assert!(!data
            .toggle_ephemeral_replies(guild_id)
            .await
            .expect("no pool, nothing to fail"));
        assert!(!data.get_ephemeral_replies(guild_id).await);

        // Settings created by the toggle belong to this guild, not a default
        // guild 0 that a save would write as its own row.
        let stored = data
            .guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|settings| settings.guild_id.clone());
        assert_eq!(stored, Some(guild_id.to_ref()));
    }

    /// A guild with no settings yet gets its own, not a default guild's, and
    /// the first toggle on it reports the value it was created with.
    #[tokio::test]
    async fn toggle_autopause_creates_settings_for_the_guild_then_flips() {
        let data = GuildData::default();
        let guild_id = GuildId::new(77);
        assert!(!data.toggle_autopause(guild_id).await);
        assert!(data.toggle_autopause(guild_id).await);
        assert!(!data.toggle_autopause(guild_id).await);
        let stored = data
            .guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map(|s| s.guild_id.clone());
        assert_eq!(stored, Some(guild_id.to_ref()));
    }

    #[tokio::test]
    async fn ephemeral_replies_follow_the_guild_setting() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);
        assert!(
            !data.get_ephemeral_replies(guild_id).await,
            "no settings means visible"
        );

        let mut settings = GuildSettings::new(guild_id.to_ref(), None, None);
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings.clone());
        assert!(
            !data.get_ephemeral_replies(guild_id).await,
            "settings with the flag off mean visible"
        );

        settings.ephemeral_replies = true;
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings);

        assert!(data.get_ephemeral_replies(guild_id).await);
    }

    #[tokio::test]
    async fn test_get_music_channel() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let channel_id = GenericChannelId::new(2);
        let mut settings = crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None);
        settings.set_music_channel(channel_id.to_ref());
        guild_settings_map.insert(guild_id, settings);
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(data.get_music_channel(guild_id).await, Some(channel_id));
    }

    #[tokio::test]
    async fn test_set_music_channel() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let channel_id = GenericChannelId::new(2);
        let mut settings = crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None);
        settings.set_music_channel(channel_id.to_ref());
        guild_settings_map.insert(guild_id, settings);
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_music_channel(guild_id, GenericChannelId::new(3))
            .await;

        assert_eq!(
            data.get_music_channel(guild_id).await,
            Some(GenericChannelId::new(3))
        );
    }

    #[tokio::test]
    async fn test_get_timeout() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                timeout: 5,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(data.get_timeout(guild_id).await, Some(5));
    }

    #[tokio::test]
    async fn test_set_timeout() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                timeout: 5,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_timeout(guild_id, 10).await;

        assert_eq!(data.get_timeout(guild_id).await, Some(10));
    }

    #[tokio::test]
    async fn test_get_premium() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                premium: true,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(data.get_premium(guild_id).await, Some(true));
    }

    #[tokio::test]
    async fn test_set_premium() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                premium: true,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_premium(guild_id, false).await;

        assert_eq!(data.get_premium(guild_id).await, Some(false));
    }

    #[tokio::test]
    async fn test_get_prefix() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                prefix: "!".to_string(),
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(data.get_prefix(guild_id).await, Some("!".to_string()));
    }

    #[tokio::test]
    async fn test_set_prefix() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                prefix: "!".to_string(),
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_prefix(guild_id, "?".to_string()).await;

        assert_eq!(data.get_prefix(guild_id).await, Some("?".to_string()));
    }

    #[tokio::test]
    async fn test_add_prefix() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                prefix: "!".to_string(),
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.add_prefix(guild_id, "?".to_string()).await;

        assert_eq!(data.get_prefix(guild_id).await, Some("!".to_string()));
        assert_eq!(
            data.get_additional_prefixes(guild_id).await,
            vec!["?".to_string()]
        );
    }

    #[tokio::test]
    async fn test_get_autopause() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                autopause: true,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert!(data.get_autopause(guild_id).await);
    }

    #[tokio::test]
    async fn test_set_autopause() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                autopause: true,
                ..crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None)
            },
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        data.set_autopause(guild_id, false).await;

        assert!(!data.get_autopause(guild_id).await);
    }

    #[tokio::test]
    async fn test_get_set_autorole() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let auto_role = 123;
        let mut settings = crate::GuildSettings::new(guild_id.to_ref(), Some("r!"), None);
        settings.set_auto_role(Some(RoleId::new(auto_role).to_ref()));
        guild_settings_map.insert(guild_id, settings);
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        };

        assert_eq!(data.get_auto_role(guild_id).await, Some(auto_role));
    }

    #[tokio::test]
    async fn set_auto_role_stores_a_snowflake_and_ignores_garbage() {
        let guild_id = GuildId::new(1);
        let data = GuildData::default();
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, GuildSettings::new(guild_id.to_ref(), None, None));

        data.set_auto_role(guild_id, 456).await;
        assert_eq!(data.get_auto_role(guild_id).await, Some(456));

        data.set_auto_role(guild_id, 0).await;
        data.set_auto_role(guild_id, u64::MAX).await;
        assert_eq!(data.get_auto_role(guild_id).await, Some(456));
    }

    /// With no pool there is nothing to overwrite and no write to guard, so the
    /// helper must succeed and leave the map exactly as it found it. This is
    /// production's configuration today, so it is the case that must not break.
    #[tokio::test]
    async fn ensure_settings_loaded_is_a_noop_without_a_pool() {
        use crate::guild::settings::Provenance;

        let guild_id = GuildId::new(1);
        let mut map = HashMap::new();
        map.insert(
            guild_id,
            GuildSettings::new(guild_id.to_ref(), Some("!"), None),
        );
        let data = GuildData {
            guild_settings_map: Arc::new(RwLock::new(map)),
            ..Default::default()
        };

        assert!(
            data.database_pool.is_none(),
            "this test needs the no-pool path"
        );

        data.ensure_settings_loaded(guild_id)
            .await
            .expect("must succeed with no pool");

        let after = data
            .get_guild_settings(guild_id)
            .await
            .expect("still present");
        assert_eq!(
            after.provenance,
            Provenance::Fallback,
            "with no pool nothing round-tripped through Postgres, so the tag must stay Fallback"
        );
        assert_eq!(after.prefix, "!", "settings must be untouched");
    }

    /// A guild absent from the map is also a no-op without a pool -- the helper
    /// must not invent an entry, because an invented entry is exactly the
    /// fallback-default value that must never be written back.
    #[tokio::test]
    async fn ensure_settings_loaded_does_not_invent_an_entry_without_a_pool() {
        let guild_id = GuildId::new(42);
        let data = GuildData::default();

        data.ensure_settings_loaded(guild_id)
            .await
            .expect("must succeed with no pool");

        assert!(
            data.get_guild_settings(guild_id).await.is_none(),
            "the helper must not create a Fallback entry that a later save() could write back"
        );
    }

    #[tokio::test]
    async fn control_echoes_are_on_for_a_guild_with_no_settings() {
        let data = GuildData::default();
        assert!(data.get_control_echoes(GuildId::new(123)).await);
    }

    #[tokio::test]
    async fn control_echoes_follow_the_guild_setting() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);
        let mut settings = GuildSettings::new(guild_id.to_ref(), None, None);
        settings.control_echoes = false;
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings);
        assert!(!data.get_control_echoes(guild_id).await);
    }

    #[tokio::test]
    async fn toggling_control_echoes_flips_it_and_reports_the_new_value() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);
        assert!(!data.toggle_control_echoes(guild_id).await.unwrap());
        assert!(!data.get_control_echoes(guild_id).await);
        assert!(data.toggle_control_echoes(guild_id).await.unwrap());
        assert!(data.get_control_echoes(guild_id).await);
    }

    #[tokio::test]
    async fn now_playing_buttons_are_on_for_a_guild_with_no_settings() {
        let data = GuildData::default();
        assert!(data.get_now_playing_buttons(GuildId::new(123)).await);
    }

    #[tokio::test]
    async fn now_playing_buttons_follow_the_guild_setting() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);
        let mut settings = GuildSettings::new(guild_id.to_ref(), None, None);
        settings.now_playing_buttons = false;
        data.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings);
        assert!(!data.get_now_playing_buttons(guild_id).await);
    }

    #[tokio::test]
    async fn toggling_now_playing_buttons_flips_it_and_reports_the_new_value() {
        let data = GuildData::default();
        let guild_id = GuildId::new(123);
        assert!(!data.toggle_now_playing_buttons(guild_id).await.unwrap());
        assert!(!data.get_now_playing_buttons(guild_id).await);
        assert!(data.toggle_now_playing_buttons(guild_id).await.unwrap());
        assert!(data.get_now_playing_buttons(guild_id).await);
    }
}
