//! `Data`'s side of the guild settings: each method hands the call to the
//! [`GuildData`](super::operations::GuildData) handle, which shares the map.

use super::operations::GuildSettingsOperations;
use crate::{errors::CrackedError, Data, GuildSettings};
use serenity::all::{GenericChannelId, GuildId};

impl GuildSettingsOperations for Data {
    async fn get_or_create_guild_settings(
        &self,
        guild_id: GuildId,
        name: Option<String>,
        prefix: Option<&str>,
    ) -> GuildSettings {
        self.guild_data()
            .get_or_create_guild_settings(guild_id, name, prefix)
            .await
    }

    async fn get_guild_settings(&self, guild_id: GuildId) -> Option<GuildSettings> {
        self.guild_data().get_guild_settings(guild_id).await
    }

    async fn set_guild_settings(
        &self,
        guild_id: GuildId,
        settings: GuildSettings,
    ) -> Option<GuildSettings> {
        self.guild_data()
            .set_guild_settings(guild_id, settings)
            .await
    }

    async fn ensure_settings_loaded(&self, guild_id: GuildId) -> Result<(), CrackedError> {
        self.guild_data().ensure_settings_loaded(guild_id).await
    }

    async fn get_music_channel(&self, guild_id: GuildId) -> Option<GenericChannelId> {
        self.guild_data().get_music_channel(guild_id).await
    }

    async fn set_music_channel(&self, guild_id: GuildId, channel_id: GenericChannelId) {
        self.guild_data()
            .set_music_channel(guild_id, channel_id)
            .await
    }

    async fn get_timeout(&self, guild_id: GuildId) -> Option<u32> {
        self.guild_data().get_timeout(guild_id).await
    }

    async fn set_timeout(&self, guild_id: GuildId, timeout: u32) {
        self.guild_data().set_timeout(guild_id, timeout).await
    }

    async fn get_premium(&self, guild_id: GuildId) -> Option<bool> {
        self.guild_data().get_premium(guild_id).await
    }

    async fn set_premium(&self, guild_id: GuildId, premium: bool) {
        self.guild_data().set_premium(guild_id, premium).await
    }

    async fn get_prefix(&self, guild_id: GuildId) -> Option<String> {
        self.guild_data().get_prefix(guild_id).await
    }

    async fn set_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_data().set_prefix(guild_id, prefix).await
    }

    async fn add_prefix(&self, guild_id: GuildId, prefix: String) {
        self.guild_data().add_prefix(guild_id, prefix).await
    }

    async fn get_additional_prefixes(&self, guild_id: GuildId) -> Vec<String> {
        self.guild_data().get_additional_prefixes(guild_id).await
    }

    async fn set_additional_prefixes(&self, guild_id: GuildId, prefixes: Vec<String>) {
        self.guild_data()
            .set_additional_prefixes(guild_id, prefixes)
            .await
    }

    async fn get_autopause(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_autopause(guild_id).await
    }

    async fn set_autopause(&self, guild_id: GuildId, autopause: bool) {
        self.guild_data().set_autopause(guild_id, autopause).await
    }

    async fn toggle_autopause(&self, guild_id: GuildId) -> bool {
        self.guild_data().toggle_autopause(guild_id).await
    }

    async fn get_auto_role(&self, guild_id: GuildId) -> Option<u64> {
        self.guild_data().get_auto_role(guild_id).await
    }

    async fn set_auto_role(&self, guild_id: GuildId, auto_role: u64) {
        self.guild_data().set_auto_role(guild_id, auto_role).await
    }

    async fn set_autoplay_setting(&self, guild_id: GuildId, autoplay: bool) {
        self.guild_data()
            .set_autoplay_setting(guild_id, autoplay)
            .await
    }

    async fn get_autoplay_setting(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_autoplay_setting(guild_id).await
    }

    async fn get_volume(&self, guild_id: GuildId) -> (f32, f32) {
        self.guild_data().get_volume(guild_id).await
    }

    async fn set_volume(&self, guild_id: GuildId, vol: f32) -> f32 {
        self.guild_data().set_volume(guild_id, vol).await
    }

    async fn get_reply_with_embed(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_reply_with_embed(guild_id).await
    }

    async fn set_reply_with_embed(&self, guild_id: GuildId, as_embed: bool) -> bool {
        self.guild_data()
            .set_reply_with_embed(guild_id, as_embed)
            .await
    }

    async fn get_ephemeral_replies(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_ephemeral_replies(guild_id).await
    }

    async fn toggle_ephemeral_replies(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.guild_data().toggle_ephemeral_replies(guild_id).await
    }

    async fn get_control_echoes(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_control_echoes(guild_id).await
    }

    async fn toggle_control_echoes(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.guild_data().toggle_control_echoes(guild_id).await
    }

    async fn get_now_playing_buttons(&self, guild_id: GuildId) -> bool {
        self.guild_data().get_now_playing_buttons(guild_id).await
    }

    async fn toggle_now_playing_buttons(&self, guild_id: GuildId) -> Result<bool, CrackedError> {
        self.guild_data().toggle_now_playing_buttons(guild_id).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crack_types::discord::ToRef;

    /// Every `Data` method must reach the handle's method of the same name.
    /// `get_autopause` and `get_autoplay_setting` are both `bool`, so a swap
    /// compiles: the literals below differ from every default to catch one.
    #[tokio::test]
    async fn data_delegates_every_setting_to_the_shared_map() {
        let data = Data::default();
        let handle = data.guild_data();
        let guild_id = GuildId::new(5);
        let music_channel = GenericChannelId::new(77);

        let mut stored = GuildSettings::new(guild_id.to_ref(), Some("z!"), Some("Zed".into()));
        stored.set_music_channel(music_channel.to_ref());
        stored.timeout = 1234;
        stored.premium = true;
        stored.additional_prefixes = vec!["a!".into(), "b!".into()];
        stored.autopause = true;
        stored.autoplay = false;
        stored.volume = 0.31;
        stored.old_volume = 0.62;
        stored.reply_with_embed = false;
        stored.ephemeral_replies = true;
        stored.control_echoes = false;
        stored.now_playing_buttons = false;
        stored.set_auto_role(Some(serenity::all::RoleId::new(321).to_ref()));
        handle.set_guild_settings(guild_id, stored.clone()).await;

        assert_eq!(
            data.get_guild_settings(guild_id).await,
            Some(stored.clone())
        );
        assert_eq!(data.get_music_channel(guild_id).await, Some(music_channel));
        assert_eq!(data.get_timeout(guild_id).await, Some(1234));
        assert_eq!(data.get_premium(guild_id).await, Some(true));
        assert_eq!(data.get_prefix(guild_id).await, Some("z!".to_string()));
        assert_eq!(
            data.get_additional_prefixes(guild_id).await,
            vec!["a!".to_string(), "b!".to_string()]
        );
        assert!(data.get_autopause(guild_id).await);
        assert_eq!(data.get_auto_role(guild_id).await, Some(321));
        assert!(!data.get_autoplay_setting(guild_id).await);
        assert_eq!(data.get_volume(guild_id).await, (0.31, 0.62));
        assert!(!data.get_reply_with_embed(guild_id).await);
        assert!(data.get_ephemeral_replies(guild_id).await);
        assert!(!data.get_control_echoes(guild_id).await);
        assert!(!data.get_now_playing_buttons(guild_id).await);
        assert_eq!(
            data.get_or_create_guild_settings(guild_id, None, None)
                .await,
            stored
        );

        // Setters and toggles, read back through the handle.
        let other = GenericChannelId::new(88);
        data.set_music_channel(guild_id, other).await;
        assert_eq!(handle.get_music_channel(guild_id).await, Some(other));
        data.set_timeout(guild_id, 99).await;
        assert_eq!(handle.get_timeout(guild_id).await, Some(99));
        data.set_premium(guild_id, false).await;
        assert_eq!(handle.get_premium(guild_id).await, Some(false));
        data.set_prefix(guild_id, "q!".to_string()).await;
        assert_eq!(handle.get_prefix(guild_id).await, Some("q!".to_string()));
        data.add_prefix(guild_id, "c!".to_string()).await;
        assert!(handle
            .get_additional_prefixes(guild_id)
            .await
            .contains(&"c!".to_string()));
        data.set_additional_prefixes(guild_id, vec!["only!".to_string()])
            .await;
        assert_eq!(
            handle.get_additional_prefixes(guild_id).await,
            vec!["only!".to_string()]
        );
        data.set_autopause(guild_id, false).await;
        assert!(!handle.get_autopause(guild_id).await);
        assert!(data.toggle_autopause(guild_id).await);
        assert!(handle.get_autopause(guild_id).await);
        data.set_auto_role(guild_id, 654).await;
        assert_eq!(handle.get_auto_role(guild_id).await, Some(654));
        data.set_autoplay_setting(guild_id, true).await;
        assert!(handle.get_autoplay_setting(guild_id).await);
        data.set_volume(guild_id, 0.5).await;
        assert_eq!(handle.get_volume(guild_id).await.0, 0.5);
        // 🪤 Reports the stored value and writes nothing, as it always has.
        assert!(!data.set_reply_with_embed(guild_id, true).await);
        assert!(!handle.get_reply_with_embed(guild_id).await);
        data.ensure_settings_loaded(guild_id)
            .await
            .expect("no pool, nothing to load");
        assert!(!data.toggle_ephemeral_replies(guild_id).await.unwrap());
        assert!(!handle.get_ephemeral_replies(guild_id).await);
        assert!(data.toggle_control_echoes(guild_id).await.unwrap());
        assert!(handle.get_control_echoes(guild_id).await);
        assert!(data.toggle_now_playing_buttons(guild_id).await.unwrap());
        assert!(handle.get_now_playing_buttons(guild_id).await);
    }

    /// 🔑 The regression this whole change exists to prevent: a NEW command that
    /// writes guild settings back but forgets the guard.
    ///
    /// Every command file that calls `GuildSettings::save` must also call
    /// `ensure_settings_loaded`. Reading the tree is the only way to assert
    /// this -- there is no type that forces it, and the failure is silent.
    #[test]
    fn every_command_that_saves_settings_also_guards_first() {
        use std::path::Path;

        fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).expect("readable") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let src = std::fs::read_to_string(&path).expect("readable");
                    out.push((path.display().to_string(), src));
                }
            }
        }

        let mut files = Vec::new();
        walk(Path::new("src/commands"), &mut files);
        assert!(!files.is_empty(), "found no command sources to scan");

        let mut unguarded = Vec::new();
        for (path, src) in &files {
            // The bug needs BOTH halves: read the in-memory settings map, then
            // write it back. A file that saves some other type (GpSaved,
            // WelcomeSettings) never touches `guild_settings_map` and so cannot
            // be this bug -- requiring both keeps the scan precise instead of
            // maintaining a list of filenames to skip.
            let saves = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(".save(&pool)") || l.contains(".save(&pg_pool)"));
            let reads_the_map =
                src.contains("guild_settings_map") || src.contains("get_guild_settings");
            if saves && reads_the_map && !src.contains("ensure_settings_loaded") {
                unguarded.push(path.clone());
            }
        }

        assert!(
            unguarded.is_empty(),
            "these command files write guild settings back without first calling \
             ensure_settings_loaded, which lets a guild whose load failed overwrite \
             its stored row with defaults: {unguarded:#?}"
        );
    }
}
