use std::{future::Future, sync::Arc};

use ::serenity::all::Context as SerenityContext;
use serenity::all::{ChannelId, GuildId};

use crate::errors::CrackedError;

pub trait GuildSettingsOperations {
    fn get_music_channel(&self, guild_id: GuildId) -> Option<ChannelId>;
    fn set_music_channel(&self, guild_id: GuildId, channel_id: ChannelId);
    fn save_guild_settings(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<(), CrackedError>>;
    fn get_timeout(&self, guild_id: GuildId) -> Option<u32>;
    fn set_timeout(&self, guild_id: GuildId, timeout: u32);
    fn get_premium(&self, guild_id: GuildId) -> Option<bool>;
    fn set_premium(&self, guild_id: GuildId, premium: bool);
    fn get_prefix(&self, guild_id: GuildId) -> Option<String>;
    fn set_prefix(&self, guild_id: GuildId, prefix: String);
    fn add_prefix(&self, guild_id: GuildId, prefix: String);
    fn get_autopause(&self, guild_id: GuildId) -> bool;
    fn set_autopause(&self, guild_id: GuildId, autopause: bool);
}

impl GuildSettingsOperations for crate::Data {
    fn get_music_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.guild_settings_map
            .read()
            .unwrap()
            .get(&guild_id)
            .map(|x| {
                x.command_channels
                    .music_channel
                    .as_ref()
                    .map(|x| x.channel_id)
            })
            .flatten()
    }

    fn set_music_channel(&self, guild_id: GuildId, channel_id: ChannelId) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.set_music_channel(channel_id.get());
            });
    }

    async fn save_guild_settings(&self, guild_id: GuildId) -> Result<(), CrackedError> {
        let opt_settings = self.guild_settings_map.read().unwrap().clone();
        let settings = opt_settings.get(&guild_id);

        let pg_pool = self.database_pool.clone().unwrap();
        settings.map(|s| s.save(&pg_pool)).unwrap().await
    }

    fn get_timeout(&self, guild_id: GuildId) -> Option<u32> {
        self.guild_settings_map
            .read()
            .unwrap()
            .get(&guild_id)
            .map(|x| x.timeout)
    }

    fn set_timeout(&self, guild_id: GuildId, timeout: u32) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.timeout = timeout;
            })
            .key();
    }

    fn get_premium(&self, guild_id: GuildId) -> Option<bool> {
        self.guild_settings_map
            .read()
            .unwrap()
            .get(&guild_id)
            .map(|x| x.premium)
    }

    fn set_premium(&self, guild_id: GuildId, premium: bool) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.premium = premium;
            });
    }

    fn get_prefix(&self, guild_id: GuildId) -> Option<String> {
        self.guild_settings_map
            .read()
            .unwrap()
            .get(&guild_id)
            .map(|x| x.prefix.clone())
    }

    fn set_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.prefix = prefix;
            });
    }

    fn add_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.prefix.push_str(&prefix);
            });
    }

    fn get_autopause(&self, guild_id: GuildId) -> bool {
        self.guild_settings_map
            .read()
            .unwrap()
            .get(&guild_id)
            .map(|x| x.autopause)
            .unwrap_or(false)
    }

    fn set_autopause(&self, guild_id: GuildId, autopause: bool) {
        self.guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .and_modify(|e| {
                e.autopause = autopause;
            });
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
    use std::sync::RwLock;

    #[test]
    fn test_get_music_channel() {
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

        assert_eq!(data.get_music_channel(guild_id), Some(channel_id));
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

        assert_eq!(data.get_music_channel(guild_id), Some(ChannelId::new(3)));
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

        assert_eq!(data.get_prefix(guild_id), Some("!?".to_string()));
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
