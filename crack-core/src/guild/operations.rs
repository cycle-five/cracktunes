use crate::{Data, GuildSettings};
use crack_types::CrackedError;
use serenity::{
    all::{ChannelId, Context as SerenityContext, GuildId},
    small_fixed_array::FixedString,
};
use std::str::FromStr;
use std::{future::Future, sync::Arc};

use super::settings::DEFAULT_VOLUME_LEVEL;

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
    fn save_guild_settings(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<(), CrackedError>>;
    fn get_music_channel(&self, guild_id: GuildId) -> impl Future<Output = Option<ChannelId>>;
    fn set_music_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
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
    fn get_autoplay(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn set_autoplay(&self, guild_id: GuildId, autoplay: bool) -> impl Future<Output = ()>;
    fn set_autoplay_setting(&self, guild_id: GuildId, autoplay: bool) -> impl Future<Output = ()>;
    fn get_autoplay_setting(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn get_volume(&self, guild_id: GuildId) -> impl Future<Output = (f32, f32)>;
    fn set_volume(&self, guild_id: GuildId, volume: u64) -> impl Future<Output = ()>;
    fn get_reply_with_embed(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn set_reply_with_embed(&self, guild_id: GuildId, as_embed: bool)
        -> impl Future<Output = bool>;
}

/// Implementation of the guild settings operations.
impl GuildSettingsOperations for Data {
    /// Get the guild settings for a guild, creating them if they don't exist.
    async fn get_or_create_guild_settings(
        &self,
        guild_id: GuildId,
        name: Option<String>,
        prefix: Option<&str>,
    ) -> GuildSettings {
        let name = name.map(|x| FixedString::from_str(x.as_str()).expect("wtf?"));
        self.get_guild_settings(guild_id).await.unwrap_or({
            let settings = GuildSettings::new(guild_id, prefix, name);
            self.set_guild_settings(guild_id, settings.clone()).await;
            settings
        })
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
    async fn get_music_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .and_then(super::settings::GuildSettings::get_music_channel)
    }

    /// Set the music channel for the guild.
    async fn set_music_channel(&self, guild_id: GuildId, channel_id: ChannelId) {
        let mut guard = self.guild_settings_map.write().await;
        let _ = guard
            .entry(guild_id)
            .and_modify(|x| x.set_music_channel(channel_id.get()))
            .or_insert_with(|| GuildSettings::new(guild_id, None, None));
    }

    /// Save the guild settings to the database.
    async fn save_guild_settings(&self, guild_id: GuildId) -> Result<(), CrackedError> {
        let opt_settings = self.guild_settings_map.read().await;
        let settings = opt_settings.get(&guild_id);

        let pg_pool = self.database_pool.clone().unwrap();
        settings.map(|s| s.save(&pg_pool)).unwrap().await
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
            .is_some_and(|x| x.autopause)
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
            .or_insert_with(Default::default)
            .autopause
    }

    /// Get the current auto role for the guild.
    async fn get_auto_role(&self, guild_id: GuildId) -> Option<u64> {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .and_then(|x| x.welcome_settings.as_ref())
            .and_then(|x| x.auto_role)
    }

    /// Set the auto role for the guild.
    async fn set_auto_role(&self, guild_id: GuildId, auto_role: u64) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.set_auto_role(Some(auto_role));
            });
    }

    /// Get the current autoplay settings.
    async fn get_autoplay(&self, guild_id: GuildId) -> bool {
        self.guild_cache_map
            .lock()
            .await
            .get(&guild_id)
            .is_none_or(|settings| settings.autoplay)
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
            .is_none_or(|e| e.autoplay)
    }

    /// Set the autoplay setting
    async fn set_autoplay(&self, guild_id: GuildId, autoplay: bool) {
        self.guild_cache_map
            .lock()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.autoplay = autoplay;
            });
    }

    /// Get the current autoplay settings.
    async fn get_volume(&self, guild_id: GuildId) -> (f32, f32) {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map_or((DEFAULT_VOLUME_LEVEL, DEFAULT_VOLUME_LEVEL), |settings| {
                (settings.volume, settings.old_volume)
            })
    }

    /// Set the current autoplay settings.
    async fn set_volume(&self, guild_id: GuildId, vol: u64) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.old_volume = e.volume;
                e.volume = vol as f32;
            })
            .or_insert_with(|| GuildSettings {
                volume: vol as f32,
                old_volume: vol as f32,
                ..Default::default()
            });
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
}

/// Get all guilds the bot is in (that are cached).
#[cfg(not(tarpaulin_include))]
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
    use crate::guild::cache::GuildCache;
    use crate::{Data, DataInner};
    use serenity::model::id::ChannelId;
    use std::collections::HashMap;
    use tokio::sync::{Mutex, RwLock};

    #[tokio::test]
    async fn test_get_guild_settings() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                ..Default::default()
            },
        );
        let data = Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        }));

        assert_eq!(
            data.get_guild_settings(guild_id).await,
            Some(crate::GuildSettings {
                ..Default::default()
            })
        );
    }

    #[tokio::test]
    async fn test_get_or_create_guild_settings() {
        let guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let data = Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        }));

        assert_eq!(
            data.get_or_create_guild_settings(guild_id, None, None)
                .await,
            crate::GuildSettings {
                guild_id,
                ..Default::default()
            }
        );
    }

    #[tokio::test]
    async fn test_set_guild_settings() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                ..Default::default()
            },
        );
        let data = Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        }));

        data.set_guild_settings(
            guild_id,
            crate::GuildSettings {
                ..Default::default()
            },
        )
        .await;

        assert_eq!(
            data.get_guild_settings(guild_id).await,
            Some(crate::GuildSettings {
                ..Default::default()
            })
        );
    }

    #[tokio::test]
    async fn test_get_music_channel() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let channel_id = ChannelId::new(2);
        let mut settings = crate::GuildSettings::default();
        settings.set_music_channel(channel_id.get());
        guild_settings_map.insert(guild_id, settings);
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        assert_eq!(data.get_music_channel(guild_id).await, Some(channel_id));
    }

    #[tokio::test]
    async fn test_set_music_channel() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let channel_id = ChannelId::new(2);
        let mut settings = crate::GuildSettings::default();
        settings.set_music_channel(channel_id.get());
        guild_settings_map.insert(guild_id, settings);
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        data.set_music_channel(guild_id, ChannelId::new(3)).await;

        assert_eq!(
            data.get_music_channel(guild_id).await,
            Some(ChannelId::new(3))
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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

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
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        data.set_autopause(guild_id, false).await;

        assert!(!(data.get_autopause(guild_id).await));
    }

    #[tokio::test]
    async fn test_get_set_autorole() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let auto_role = 123;
        let mut settings = crate::GuildSettings::default();
        settings.set_auto_role(Some(auto_role));
        guild_settings_map.insert(guild_id, settings);
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        assert_eq!(data.get_auto_role(guild_id).await, Some(auto_role));
    }

    #[tokio::test]
    async fn test_get_set_autoplay() {
        let mut guild_cache_map = HashMap::new();
        let guild_id = GuildId::new(1);
        guild_cache_map.insert(
            guild_id,
            GuildCache {
                autoplay: false,
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_cache_map: Arc::new(Mutex::new(guild_cache_map)),
            ..Default::default()
        })));

        assert!(!(data.get_autoplay(guild_id).await));
    }
}
