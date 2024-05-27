use crate::{errors::CrackedError, Data, GuildSettings};
use serenity::all::{ChannelId, Context as SerenityContext, GuildId};
use std::{future::Future, sync::Arc};

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
    fn get_autoplay(&self, guild_id: GuildId) -> impl Future<Output = bool>;
    fn set_autoplay(&self, guild_id: GuildId, autoplay: bool) -> impl Future<Output = ()>;
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
            .and_then(|x| {
                x.command_channels
                    .music_channel
                    .as_ref()
                    .map(|x| x.channel_id)
            })
    }

    /// Set the music channel for the guild.
    async fn set_music_channel(&self, guild_id: GuildId, channel_id: ChannelId) {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .and_modify(|e| {
                e.set_music_channel(channel_id.get());
            });
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

    /// Get the current autoplay settings.
    async fn get_autoplay(&self, guild_id: GuildId) -> bool {
        self.guild_cache_map
            .lock()
            .await
            .get(&guild_id)
            .map(|settings| settings.autoplay)
            .unwrap_or(true)
    }

    /// Set the autoplay setting
    async fn set_autoplay(&self, guild_id: GuildId, autoplay: bool) {
        self.guild_cache_map
            .lock()
            .await
            .entry(guild_id)
            .or_default()
            .autoplay = autoplay;
    }

    /// Get the current reply with embed setting.
    async fn get_reply_with_embed(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .await
            .get(&guild_id)
            .map_or(true, |x| x.reply_with_embed)
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
    use crate::{
        guild::{permissions::CommandChannel, settings::CommandChannels},
        Data, DataInner,
    };
    use serenity::model::id::ChannelId;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

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
        );

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
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                command_channels: CommandChannels {
                    music_channel: Some(CommandChannel {
                        command: "".to_string(),
                        guild_id,
                        channel_id,
                        permission_settings: Default::default(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        assert_eq!(data.get_music_channel(guild_id).await, Some(channel_id));
    }

    #[test]
    fn test_set_music_channel() {
        let mut guild_settings_map = HashMap::new();
        let guild_id = GuildId::new(1);
        let channel_id = ChannelId::new(2);
        guild_settings_map.insert(
            guild_id,
            crate::GuildSettings {
                command_channels: CommandChannels {
                    music_channel: Some(CommandChannel {
                        command: "".to_string(),
                        guild_id,
                        channel_id,
                        permission_settings: Default::default(),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let data = Arc::new(Data(Arc::new(DataInner {
            guild_settings_map: Arc::new(RwLock::new(guild_settings_map)),
            ..Default::default()
        })));

        data.set_music_channel(guild_id, ChannelId::new(3));

        assert_eq!(data.get_music_channel(guild_id).await, Some(ChannelId::new(3)));
    }

    #[test]
    fn test_get_timeout() {
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

        assert_eq!(data.get_timeout(guild_id), Some(5));
    }

    #[test]
    fn test_set_timeout() {
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

        data.set_timeout(guild_id, 10);

        assert_eq!(data.get_timeout(guild_id), Some(10));
    }

    #[test]
    fn test_get_premium() {
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

        assert_eq!(data.get_premium(guild_id), Some(true));
    }

    #[test]
    fn test_set_premium() {
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

        data.set_premium(guild_id, false);

        assert_eq!(data.get_premium(guild_id), Some(false));
    }

    #[test]
    fn test_get_prefix() {
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

        assert_eq!(data.get_prefix(guild_id), Some("!".to_string()));
    }

    #[test]
    fn test_set_prefix() {
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

        data.set_prefix(guild_id, "?".to_string());

        assert_eq!(data.get_prefix(guild_id), Some("?".to_string()));
    }

    #[test]
    fn test_add_prefix() {
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

        data.add_prefix(guild_id, "?".to_string());

        assert_eq!(data.get_prefix(guild_id), Some("!".to_string()));
        assert_eq!(
            data.get_additional_prefixes(guild_id),
            vec!["?".to_string()]
        );
    }

    #[test]
    fn test_get_autopause() {
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

        assert_eq!(data.get_autopause(guild_id), true);
    }

    #[test]
    fn test_set_autopause() {
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

        data.set_autopause(guild_id, false);

        assert_eq!(data.get_autopause(guild_id), false);
    }
}
