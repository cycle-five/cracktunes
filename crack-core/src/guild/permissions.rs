use crack_model::{CommunityRef, Kind, Ref, RoleRef, RoomRef, UserRef};
use crack_types::discord::{as_snowflake, as_snowflake_set};
use serde::{Deserialize, Serialize};
use serenity::all::GuildId;
use sqlx::{FromRow, PgPool};
use std::collections::HashSet;

use crate::{
    errors::CrackedError,
    guild::settings::{column_i64, ref_from_column, stored_ref},
    Error,
};

/// Type alias for a HashSet of strings.
type HashSetString = HashSet<String>;

/// Trait for converting a serde_json::Value to a HashSet of strings.
pub trait ConvertToHashSetString {
    fn convert(self) -> HashSetString;
}

/// Implementation of ConvertToHashSetString for serde_json::Value.
impl ConvertToHashSetString for serde_json::Value {
    fn convert(self) -> HashSetString {
        self.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }
}

/// Implementation of ConvertToHashSetString for `Vec<String>`.
impl ConvertToHashSetString for Vec<String> {
    fn convert(self) -> HashSetString {
        self.into_iter().collect()
    }
}

/// Convert a stored `BIGINT[]` to a set of refs. An element that is not a
/// snowflake is dropped with a warning, never panicked on.
pub(crate) fn ref_set<K: Kind + std::hash::Hash + Eq>(
    column: &'static str,
    raw: Vec<i64>,
) -> HashSet<Ref<K>> {
    raw.into_iter()
        .filter_map(|x| stored_ref(column, Some(x)))
        .collect()
}

/// The `BIGINT[]` to store for a set of refs.
fn to_column<K: Kind>(set: &HashSet<Ref<K>>) -> Result<Vec<i64>, CrackedError> {
    set.iter().map(column_i64).collect()
}

/// Struct for generic permission settings. Includes allowed and denied commands, roles, and users.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenericPermissionSettings {
    pub id: i64,
    #[serde(default = "default_true")]
    pub default_allow_all_commands: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_users: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_roles: bool,
    #[serde(with = "as_snowflake_set")]
    pub allowed_roles: HashSet<RoleRef>,
    #[serde(with = "as_snowflake_set")]
    pub denied_roles: HashSet<RoleRef>,
    #[serde(with = "as_snowflake_set")]
    pub allowed_users: HashSet<UserRef>,
    #[serde(with = "as_snowflake_set")]
    pub denied_users: HashSet<UserRef>,
    #[serde(default, with = "as_snowflake_set")]
    pub allowed_channels: HashSet<RoomRef>,
    #[serde(default, with = "as_snowflake_set")]
    pub denied_channels: HashSet<RoomRef>,
    // pub allowed_commands: HashSet<String>,
    // pub denied_commands: HashSet<String>,
}

impl From<GenericPermissionSettingsReadWCommand> for GenericPermissionSettings {
    fn from(read: GenericPermissionSettingsReadWCommand) -> Self {
        Self {
            id: read.id,
            default_allow_all_commands: read.default_allow_all_commands,
            default_allow_all_users: read.default_allow_all_users,
            default_allow_all_roles: read.default_allow_all_roles,
            allowed_roles: ref_set("permission_settings.allowed_roles", read.allowed_roles),
            denied_roles: ref_set("permission_settings.denied_roles", read.denied_roles),
            allowed_users: ref_set("permission_settings.allowed_users", read.allowed_users),
            denied_users: ref_set("permission_settings.denied_users", read.denied_users),
            allowed_channels: ref_set(
                "permission_settings.allowed_channels",
                read.allowed_channels,
            ),
            denied_channels: ref_set("permission_settings.denied_channels", read.denied_channels),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GenericPermissionSettingsReadWCommand {
    pub command: String,
    pub id: i64,
    #[serde(default = "default_true")]
    pub default_allow_all_commands: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_users: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_roles: bool,
    // pub allowed_commands: serde_json::Value,
    // pub denied_commands: serde_json::Value,
    pub allowed_roles: Vec<i64>,
    pub denied_roles: Vec<i64>,
    pub allowed_users: Vec<i64>,
    pub denied_users: Vec<i64>,
    pub allowed_channels: Vec<i64>,
    pub denied_channels: Vec<i64>,
}

/// Struct for reading generic permission settings from a pg table.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GenericPermissionSettingsRead {
    pub id: i64,
    #[serde(default = "default_true")]
    pub default_allow_all_commands: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_users: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_roles: bool,
    // pub allowed_commands: serde_json::Value,
    // pub denied_commands: serde_json::Value,
    pub allowed_roles: Vec<i64>,
    pub denied_roles: Vec<i64>,
    pub allowed_users: Vec<i64>,
    pub denied_users: Vec<i64>,
    pub allowed_channels: Vec<i64>,
    pub denied_channels: Vec<i64>,
}

/// Implementation of GenericPermissionSettingsRead.
impl GenericPermissionSettingsRead {
    pub fn convert(self) -> GenericPermissionSettings {
        GenericPermissionSettings {
            id: self.id,
            default_allow_all_commands: self.default_allow_all_commands,
            default_allow_all_users: self.default_allow_all_users,
            default_allow_all_roles: self.default_allow_all_roles,
            // allowed_commands: ConvertToHashSetString::convert(self.allowed_commands),
            // denied_commands: ConvertToHashSetString::convert(self.denied_commands),
            allowed_roles: ref_set("permission_settings.allowed_roles", self.allowed_roles),
            denied_roles: ref_set("permission_settings.denied_roles", self.denied_roles),
            allowed_users: ref_set("permission_settings.allowed_users", self.allowed_users),
            denied_users: ref_set("permission_settings.denied_users", self.denied_users),
            allowed_channels: ref_set(
                "permission_settings.allowed_channels",
                self.allowed_channels,
            ),
            denied_channels: ref_set("permission_settings.denied_channels", self.denied_channels),
        }
    }
}

/// Default true for serialization
fn default_true() -> bool {
    true
}

/// Default implementation for GenericPermissionSettings.
impl Default for GenericPermissionSettings {
    fn default() -> Self {
        Self {
            id: 0,
            default_allow_all_commands: true,
            default_allow_all_users: true,
            default_allow_all_roles: true,
            // allowed_commands: HashSet::new(),
            // denied_commands: HashSet::new(),
            allowed_roles: HashSet::new(),
            denied_roles: HashSet::new(),
            allowed_users: HashSet::new(),
            denied_users: HashSet::new(),
            allowed_channels: HashSet::new(),
            denied_channels: HashSet::new(),
        }
    }
}

/// Implementation of GenericPermissionSettings.
/// The behavior of this ACL is as follows:
/// - If both white and black lists are empty, all commands are allowed.
/// - If a command is in the denied commands, all other commands are allowed unless default_allow_all_commands is false.
/// - If a command is in the allowed commands, all other commands are denied unless default_allow_all_commands is true.
impl GenericPermissionSettings {
    // Check if a command is allowed by the permission settings.
    // pub fn is_command_allowed(&self, command: &str) -> bool {
    //     (self.allowed_commands.is_empty()
    //         && self.denied_commands.is_empty()
    //         && self.default_allow_all_commands)
    //         || self.allowed_commands.is_empty()
    //             && self.default_allow_all_commands
    //             && !self.denied_commands.contains(command)
    //         || self.allowed_commands.contains(command) && !self.denied_commands.contains(command)
    // }

    /// Check if a role is allowed by the permission settings.
    pub fn is_role_allowed(&self, role: &RoleRef) -> bool {
        (self.allowed_roles.is_empty()
            && self.denied_roles.is_empty()
            && self.default_allow_all_roles)
            || self.default_allow_all_roles
                && self.allowed_roles.is_empty()
                && !self.denied_roles.contains(role)
            || self.allowed_roles.contains(role) && !self.denied_roles.contains(role)
    }

    /// Check if a user is allowed by the permission settings.
    pub fn is_user_allowed(&self, user: &UserRef) -> bool {
        (self.allowed_users.is_empty()
            && self.denied_users.is_empty()
            && self.default_allow_all_users)
            || self.default_allow_all_users
                && self.allowed_users.is_empty()
                && !self.denied_users.contains(user)
            || self.allowed_users.contains(user) && !self.denied_users.contains(user)
    }

    pub fn is_channel_allowed(&self, channel: &RoomRef) -> bool {
        (self.allowed_channels.is_empty() && self.denied_channels.is_empty())
            || (self.allowed_channels.is_empty() && !self.denied_channels.contains(channel))
            || (self.allowed_channels.contains(channel) && !self.denied_channels.contains(channel))
    }

    // /// Add a command to the allowed commands.
    // pub fn add_allowed_command(&mut self, command: String) {
    //     self.allowed_commands.insert(command);
    // }

    // /// Remove a command from the allowed commands.
    // pub fn remove_allowed_command(&mut self, command: &str) {
    //     self.allowed_commands.remove(command);
    // }

    // /// Add a command to the denied commands.
    // pub fn add_denied_command(&mut self, command: String) {
    //     self.denied_commands.insert(command);
    // }

    // /// Remove a command from the denied commands.
    // pub fn remove_denied_command(&mut self, command: &str) {
    //     self.denied_commands.remove(command);
    // }

    /// Add a role to the allowed roles.
    pub fn add_allowed_role(&mut self, role: RoleRef) -> bool {
        self.allowed_roles.insert(role)
    }

    /// Remove a role from the allowed roles.
    pub fn remove_allowed_role(&mut self, role: &RoleRef) -> bool {
        self.allowed_roles.remove(role)
    }

    /// Add a role to the denied roles.
    pub fn add_denied_role(&mut self, role: RoleRef) -> bool {
        self.denied_roles.insert(role)
    }

    /// Remove a role from the denied roles.
    pub fn remove_denied_role(&mut self, role: &RoleRef) -> bool {
        self.denied_roles.remove(role)
    }

    /// Add a user to the allowed users.
    pub fn add_allowed_user(&mut self, user: UserRef) -> bool {
        self.allowed_users.insert(user)
    }

    /// Remove a user from the allowed users.
    pub fn remove_allowed_user(&mut self, user: &UserRef) -> bool {
        self.allowed_users.remove(user)
    }

    /// Add a user to the denied users.
    pub fn add_denied_user(&mut self, user: UserRef) -> bool {
        self.denied_users.insert(user)
    }

    /// Remove a user from the denied users.
    pub fn remove_denied_user(&mut self, user: &UserRef) -> bool {
        self.denied_users.remove(user)
    }

    /// Add a channel to the allowed channels.
    pub fn add_allowed_channel(&mut self, channel: RoomRef) -> bool {
        self.allowed_channels.insert(channel)
    }

    /// Remove a channel from the allowed channels.
    pub fn remove_allowed_channel(&mut self, channel: &RoomRef) -> bool {
        self.allowed_channels.remove(channel)
    }

    /// Add a channel to the denied channels.
    pub fn add_denied_channel(&mut self, channel: RoomRef) -> bool {
        self.denied_channels.insert(channel)
    }

    /// Remove a channel from the denied channels.
    pub fn remove_denied_channel(&mut self, channel: &RoomRef) -> bool {
        self.denied_channels.remove(channel)
    }

    /// Clear all allowed and denied commands, roles, and users.
    pub fn clear(&mut self) {
        // self.allowed_commands.clear();
        // self.denied_commands.clear();
        self.allowed_roles.clear();
        self.denied_roles.clear();
        self.allowed_users.clear();
        self.denied_users.clear();
        self.allowed_channels.clear();
        self.denied_channels.clear();
    }

    /// Write to a pg table.
    pub async fn insert_permission_settings(
        &self,
        pool: &PgPool,
    ) -> Result<GenericPermissionSettings, CrackedError> {
        sqlx::query_as!(
            GenericPermissionSettingsRead,
            "INSERT INTO permission_settings
                (default_allow_all_commands,
                    default_allow_all_users,
                    default_allow_all_roles,
                    allowed_roles,
                    denied_roles,
                    allowed_users,
                    denied_users,
                    allowed_channels,
                    denied_channels)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *",
            self.default_allow_all_commands,
            self.default_allow_all_users,
            self.default_allow_all_roles,
            // json!(settings.allowed_commands) as serde_json::Value, // Convert to JSON
            // json!(settings.denied_commands) as serde_json::Value,
            &to_column(&self.allowed_roles)?,
            &to_column(&self.denied_roles)?,
            &to_column(&self.allowed_users)?,
            &to_column(&self.denied_users)?,
            &to_column(&self.allowed_channels)?,
            &to_column(&self.denied_channels)?,
        )
        .fetch_one(pool)
        .await
        .map(|read| read.convert())
        .map_err(Into::into)
    }

    /// Read from a pg table.
    pub async fn get_permission_settings(
        pool: &PgPool,
        id: i32,
    ) -> Result<GenericPermissionSettings, CrackedError> {
        sqlx::query_as!(
            GenericPermissionSettingsRead,
            "SELECT * FROM permission_settings WHERE id = $1",
            id
        )
        .fetch_one(pool)
        .await
        .map(|read| read.convert())
        .map_err(Into::into)
    }
}

/// Struct for a command channel with permission settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandChannel {
    pub command: String,
    #[serde(with = "as_snowflake")]
    pub channel_id: RoomRef,
    #[serde(with = "as_snowflake")]
    pub guild_id: CommunityRef,
    pub permission_settings: GenericPermissionSettings,
}

/// Struct for reading a command channel with permission settings from a pg table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct CommandChannelRead {
    pub command: String,
    pub channel_id: i64,
    pub guild_id: i64,
    pub permission_settings_id: i64,
}

impl CommandChannel {
    /// Convert a CommandChannelRead to a CommandChannel.
    pub async fn from_command_channel_read(
        pool: &PgPool,
        read: CommandChannelRead,
    ) -> Result<Self, CrackedError> {
        let perms = GenericPermissionSettings::get_permission_settings(
            pool,
            read.permission_settings_id as i32,
        )
        .await?;
        Ok(Self {
            command: read.command,
            channel_id: ref_from_column(read.channel_id)?,
            guild_id: ref_from_column(read.guild_id)?,
            permission_settings: perms,
        })
    }

    /// Insert a CommandChannel into a pg table.
    pub async fn insert_command_channel(&self, pool: &PgPool) -> Result<CommandChannel, Error> {
        let settings = if self.permission_settings.id == 0 {
            self.permission_settings
                .insert_permission_settings(pool)
                .await?
        } else {
            self.permission_settings.clone()
        };
        let command_channel = sqlx::query_as!(
            CommandChannelRead,
            r#"INSERT INTO command_channel
                (command, guild_id, channel_id, permission_settings_id)
            VALUES
                ($1, $2, $3, $4)
            ON CONFLICT (command, guild_id, channel_id) DO UPDATE
                SET permission_settings_id = $4
                WHERE command_channel.channel_id = $3 AND command_channel.guild_id = $2 AND command_channel.command = $1
            RETURNING *
            "#,
            self.command,
            column_i64(&self.guild_id)?,
            column_i64(&self.channel_id)?,
            settings.id,
        )
        .fetch_one(pool)
        .await?;
        CommandChannel::from_command_channel_read(pool, command_channel)
            .await
            .map_err(Into::into)
    }

    pub async fn save(&self, pool: &PgPool) -> Result<CommandChannel, Error> {
        self.insert_command_channel(pool).await
    }

    pub async fn get_command_channels(
        pool: &PgPool,
        command: String,
        guild_id: GuildId,
    ) -> Vec<Self> {
        let read = sqlx::query_as!(
            CommandChannelRead,
            "SELECT * FROM command_channel WHERE command = $1 AND guild_id = $2",
            command,
            guild_id.get() as i64
        )
        .fetch_all(pool)
        .await;
        let read = match read {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        let mut channels = Vec::new();
        for r in read {
            let channel = CommandChannel::from_command_channel_read(pool, r)
                .await
                .unwrap();
            channels.push(channel);
        }

        channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_types::discord::ToRef;
    use serde_json::json;
    use serenity::all::{GenericChannelId, RoleId, UserId};
    use sqlx::PgPool;

    fn role(n: u64) -> RoleRef {
        RoleId::new(n).to_ref()
    }
    fn user(n: u64) -> UserRef {
        UserId::new(n).to_ref()
    }
    fn room(n: u64) -> RoomRef {
        GenericChannelId::new(n).to_ref()
    }

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    // #[test]
    // fn test_is_command_allowed() {
    //     let mut settings = GenericPermissionSettings::default();
    //     settings.add_allowed_command("test".to_string());
    //     assert!(settings.is_command_allowed("test"));
    //     assert!(!settings.is_command_allowed("test2"));
    //     settings.add_denied_command("test".to_string());
    //     assert!(!settings.is_command_allowed("test"));
    //     assert!(!settings.is_command_allowed("test2"));
    // }

    #[test]
    fn test_is_role_allowed() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_role(role(1));
        assert!(settings.is_role_allowed(&role(1)));
        assert!(!settings.is_role_allowed(&role(2)));
        settings.add_denied_role(role(1));
        assert!(!settings.is_role_allowed(&role(1)));
    }

    #[test]
    fn test_is_user_allowed() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_user(user(1));
        assert!(settings.is_user_allowed(&user(1)));
        assert!(!settings.is_user_allowed(&user(2)));
        settings.add_denied_user(user(1));
        assert!(!settings.is_user_allowed(&user(1)));
    }

    // #[test]
    // fn test_add_remove_allowed_command() {
    //     let mut settings = GenericPermissionSettings::default();
    //     settings.add_allowed_command("test".to_string());
    //     assert!(settings.is_command_allowed("test"));
    //     settings.remove_allowed_command("test");
    //     assert!(settings.is_command_allowed("test"));
    //     settings.add_allowed_command("test2".to_string());
    //     assert!(!settings.is_command_allowed("test"));
    // }

    // #[test]
    // fn test_add_remove_denied_command() {
    //     let mut settings = GenericPermissionSettings::default();
    //     settings.add_denied_command("test".to_string());
    //     assert!(!settings.is_command_allowed("test"));
    //     settings.remove_denied_command("test");
    //     assert!(settings.is_command_allowed("test"));
    // }

    #[test]
    fn test_add_remove_allowed_role() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_role(role(1));
        assert!(settings.is_role_allowed(&role(1)));
        settings.remove_allowed_role(&role(1));
        assert!(settings.is_role_allowed(&role(1)));
        settings.add_allowed_role(role(2));
        assert!(!settings.is_role_allowed(&role(1)));
    }

    #[test]
    fn test_add_remove_denied_role() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_denied_role(role(1));
        assert!(!settings.is_role_allowed(&role(1)));
        settings.remove_denied_role(&role(1));
        assert!(settings.is_role_allowed(&role(1)));
        settings.add_denied_role(role(2));
        assert!(settings.is_role_allowed(&role(1)));
    }

    #[test]
    fn test_add_remove_allowed_user() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_user(user(1));
        assert!(settings.is_user_allowed(&user(1)));
        settings.remove_allowed_user(&user(1));
        assert!(settings.is_user_allowed(&user(1)));
    }

    #[test]
    fn test_add_remove_denied_user() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_denied_user(user(1));
        assert!(!settings.is_user_allowed(&user(1)));
        settings.remove_denied_user(&user(1));
        assert!(settings.is_user_allowed(&user(1)));
    }

    #[test]
    fn test_clear() {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test".to_string());
        settings.add_allowed_role(role(1));
        settings.add_denied_role(role(1));
        settings.add_allowed_user(user(1));
        settings.add_denied_user(user(1));
        settings.clear();
        // assert!(settings.is_command_allowed("test"));
        assert!(settings.is_role_allowed(&role(1)));
        assert!(settings.is_user_allowed(&user(1)));
    }

    #[test]
    fn test_convert() {
        let settings_read = GenericPermissionSettingsRead {
            id: 1,
            default_allow_all_commands: true,
            default_allow_all_users: true,
            default_allow_all_roles: true,
            // allowed_commands: json!(["test"]),
            // denied_commands: json!(["test2"]),
            allowed_roles: vec![1, 2],
            denied_roles: vec![1],
            allowed_users: vec![1, 2],
            denied_users: vec![1],
            allowed_channels: vec![1, 2],
            denied_channels: vec![1],
        };
        let settings = settings_read.convert();
        // assert!(settings.is_command_allowed("test"));
        assert!(!settings.is_role_allowed(&role(1)));
        assert!(!settings.is_user_allowed(&user(1)));
        assert!(!settings.is_channel_allowed(&room(1)));
        assert!(settings.is_role_allowed(&role(2)));
        assert!(settings.is_user_allowed(&user(2)));
        assert!(settings.is_channel_allowed(&room(2)));
    }

    #[test]
    fn test_convert_to_hash_set_string() {
        let value = json!(["test", "test2"]);
        let hash_set: HashSet<String> = ConvertToHashSetString::convert(value);
        assert!(hash_set.contains("test"));
        assert!(hash_set.contains("test2"));
    }

    #[test]
    fn music_permissions_still_deny_a_denied_user() {
        let mut settings = GenericPermissionSettings {
            default_allow_all_users: true,
            ..Default::default()
        };
        settings.denied_users.insert(UserId::new(7).to_ref());
        assert!(!settings.is_user_allowed(&UserId::new(7).to_ref()));
        assert!(settings.is_user_allowed(&UserId::new(8).to_ref()));

        let read = GenericPermissionSettingsRead {
            id: 1,
            default_allow_all_commands: true,
            default_allow_all_users: true,
            default_allow_all_roles: true,
            allowed_roles: vec![],
            denied_roles: vec![],
            allowed_users: vec![],
            denied_users: vec![7, 0],
            allowed_channels: vec![],
            denied_channels: vec![],
        };
        let converted = read.convert();
        assert_eq!(converted.denied_users, HashSet::from([user(7)]));
    }

    #[test]
    fn a_channel_is_allowed_by_the_lists_it_is_on() {
        let mut s = GenericPermissionSettings::default();
        assert!(s.is_channel_allowed(&room(1)), "empty lists allow all");
        s.add_denied_channel(room(1));
        assert!(!s.is_channel_allowed(&room(1)), "denied");
        assert!(s.is_channel_allowed(&room(2)), "others pass a deny list");
        s.add_allowed_channel(room(2));
        assert!(s.is_channel_allowed(&room(2)), "allowed");
        assert!(!s.is_channel_allowed(&room(3)), "others fail an allow list");
        s.add_denied_channel(room(2));
        assert!(!s.is_channel_allowed(&room(2)), "denied beats allowed");
    }

    #[test]
    fn removing_reports_whether_the_ref_was_there() {
        let mut s = GenericPermissionSettings::default();
        assert!(!s.remove_allowed_user(&user(1)));
        s.add_allowed_user(user(1));
        assert!(s.remove_allowed_user(&user(1)));
        assert!(!s.remove_allowed_user(&user(1)));
        assert!(!s.remove_allowed_channel(&room(1)));
        s.add_allowed_channel(room(1));
        assert!(s.remove_allowed_channel(&room(1)));
        assert!(!s.remove_denied_channel(&room(1)));
        s.add_denied_channel(room(1));
        assert!(s.remove_denied_channel(&room(1)));
        assert!(!s.remove_denied_channel(&room(1)));
    }

    #[test]
    fn a_command_row_converts_to_its_ref_sets() {
        let row = GenericPermissionSettingsReadWCommand {
            command: "music".into(),
            id: 4,
            default_allow_all_commands: false,
            default_allow_all_users: true,
            default_allow_all_roles: false,
            allowed_roles: vec![1],
            denied_roles: vec![2],
            allowed_users: vec![3],
            denied_users: vec![4, -1],
            allowed_channels: vec![5],
            denied_channels: vec![6],
        };
        let s = GenericPermissionSettings::from(row);
        assert_eq!(s.id, 4);
        assert!(!s.default_allow_all_commands);
        assert!(s.default_allow_all_users);
        assert!(!s.default_allow_all_roles);
        assert_eq!(s.allowed_roles, HashSet::from([role(1)]));
        assert_eq!(s.denied_roles, HashSet::from([role(2)]));
        assert_eq!(s.allowed_users, HashSet::from([user(3)]));
        assert_eq!(s.denied_users, HashSet::from([user(4)]));
        assert_eq!(s.allowed_channels, HashSet::from([room(5)]));
        assert_eq!(s.denied_channels, HashSet::from([room(6)]));
    }

    #[test]
    fn permission_sets_serialise_as_arrays_of_numbers() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_role(role(5));
        settings.add_denied_user(user(7));
        settings.add_allowed_channel(room(9));
        let v = serde_json::to_value(&settings).unwrap();
        assert_eq!(v["allowed_roles"], json!([5]));
        assert_eq!(v["denied_users"], json!([7]));
        assert_eq!(v["allowed_channels"], json!([9]));
        assert_eq!(
            serde_json::from_value::<GenericPermissionSettings>(v).unwrap(),
            settings
        );
    }

    #[test]
    fn a_command_channel_serialises_its_ids_as_numbers() {
        let channel = CommandChannel {
            command: "music".into(),
            channel_id: room(3),
            guild_id: GuildId::new(4).to_ref(),
            permission_settings: GenericPermissionSettings::default(),
        };
        let v = serde_json::to_value(&channel).unwrap();
        assert_eq!(v["channel_id"], 3);
        assert_eq!(v["guild_id"], 4);
        assert_eq!(
            serde_json::from_value::<CommandChannel>(v).unwrap(),
            channel
        );
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_insert_permission_settings(pool: PgPool) {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test2".to_string());
        settings.add_allowed_role(role(1));
        settings.add_allowed_user(user(1));
        settings.insert_permission_settings(&pool).await.unwrap();

        let settings_read = GenericPermissionSettings::get_permission_settings(&pool, 1)
            .await
            .unwrap();
        assert!(settings_read.id != settings.id);
        assert!(settings_read.default_allow_all_commands == settings.default_allow_all_commands);
        assert!(settings_read.default_allow_all_users == settings.default_allow_all_users);
        assert!(settings_read.default_allow_all_roles == settings.default_allow_all_roles);
        assert!(settings_read.allowed_roles == settings.allowed_roles);
        assert!(settings_read.denied_roles == settings.denied_roles);
        assert!(settings_read.allowed_users == settings.allowed_users);
        assert!(settings_read.denied_users == settings.denied_users);
        // assert!(settings_read.is_command_allowed("test"));
        // assert!(!settings_read.is_command_allowed("test2"));
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_insert_command_channel(pool: PgPool) {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test2".to_string());
        settings.add_allowed_role(role(1));
        settings.add_allowed_user(user(1));
        let channel = CommandChannel {
            permission_settings: settings,
            channel_id: room(1),
            guild_id: GuildId::new(1).to_ref(),
            command: "test".to_string(),
        };
        channel.insert_command_channel(&pool).await.unwrap();

        let channel_read =
            CommandChannel::get_command_channels(&pool, "test".to_string(), GuildId::new(1)).await;
        // assert!(channel_read.permission_settings.is_command_allowed("test"));
        // assert!(!channel_read.permission_settings.is_command_allowed("test2"));
        assert!(channel_read.len() == 1);
    }
}
