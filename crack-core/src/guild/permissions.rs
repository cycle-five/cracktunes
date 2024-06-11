use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GuildId};
use sqlx::{FromRow, PgPool};
use std::collections::HashSet;

use crate::{errors::CrackedError, Context, Error};

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

/// Type alias for a HashSet of u64.
type HashSetU64 = HashSet<u64>;

/// Trait for converting a serde_json::Value to a HashSet of u64.
pub trait ConvertToHashSetU64 {
    fn convert(self) -> HashSetU64;
}

/// Implementation of ConvertToHashSetU64 for serde_json::Value.
impl ConvertToHashSetU64 for serde_json::Value {
    fn convert(self) -> HashSetU64 {
        self.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect()
    }
}

/// Implementation of ConvertToHashSetU64 for `Vec<i64>`.
impl ConvertToHashSetU64 for Vec<i64> {
    fn convert(self) -> HashSetU64 {
        self.iter().map(|&x| x as u64).collect()
    }
}
/// Struct for generic permission settings. Includes allowed and denied commands, roles, and users.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct GenericPermissionSettings {
    pub id: i64,
    #[serde(default = "default_true")]
    pub default_allow_all_commands: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_users: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_roles: bool,
    pub allowed_roles: HashSet<u64>,
    pub denied_roles: HashSet<u64>,
    pub allowed_users: HashSet<u64>,
    pub denied_users: HashSet<u64>,
    // pub allowed_channels: HashSet<u64>,
    // pub denied_channels: HashSet<u64>,
    // pub allowed_commands: HashSet<String>,
    // pub denied_commands: HashSet<String>,
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
            allowed_roles: self.allowed_roles.convert(),
            denied_roles: self.denied_roles.convert(),
            allowed_users: self.allowed_users.convert(),
            denied_users: self.denied_users.convert(),
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
        }
    }
}

/// Implementation of GenericPermissionSettings.
/// The behavior of this ACL is as follows:
/// - If both white and black lists are empty, all commands are allowed.
/// - If a command is in the denied commands, all other commands are allowed unless default_allow_all_commands is false.
/// - If a command is in the allowed commands, all other commands are denied unless default_allow_all_commands is true.
impl GenericPermissionSettings {
    /// Check if a command is allowed by the permission settings.
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
    pub fn is_role_allowed(&self, role: u64) -> bool {
        (self.allowed_roles.is_empty()
            && self.denied_roles.is_empty()
            && self.default_allow_all_roles)
            || self.default_allow_all_roles
                && self.allowed_roles.is_empty()
                && !self.denied_roles.contains(&role)
            || self.allowed_roles.contains(&role) && !self.denied_roles.contains(&role)
    }

    /// Check if a user is allowed by the permission settings.
    pub fn is_user_allowed(&self, user: u64) -> bool {
        (self.allowed_users.is_empty()
            && self.denied_users.is_empty()
            && self.default_allow_all_users)
            || self.default_allow_all_users
                && self.allowed_users.is_empty()
                && !self.denied_users.contains(&user)
            || self.allowed_users.contains(&user) && !self.denied_users.contains(&user)
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
    pub fn add_allowed_role(&mut self, role: u64) -> bool {
        self.allowed_roles.insert(role)
    }

    /// Remove a role from the allowed roles.
    pub fn remove_allowed_role(&mut self, role: u64) -> bool {
        self.allowed_roles.remove(&role)
    }

    /// Add a role to the denied roles.
    pub fn add_denied_role(&mut self, role: u64) -> bool {
        self.denied_roles.insert(role)
    }

    /// Remove a role from the denied roles.
    pub fn remove_denied_role(&mut self, role: u64) -> bool {
        self.denied_roles.remove(&role)
    }

    /// Add a user to the allowed users.
    pub fn add_allowed_user(&mut self, user: u64) -> bool {
        self.allowed_users.insert(user)
    }

    /// Remove a user from the allowed users.
    pub fn remove_allowed_user(&mut self, user: u64) -> bool {
        self.allowed_users.remove(&user)
    }

    /// Add a user to the denied users.
    pub fn add_denied_user(&mut self, user: u64) -> bool {
        self.denied_users.insert(user)
    }

    /// Remove a user from the denied users.
    pub fn remove_denied_user(&mut self, user: u64) -> bool {
        self.denied_users.remove(&user)
    }

    /// Clear all allowed and denied commands, roles, and users.
    pub fn clear(&mut self) -> () {
        // self.allowed_commands.clear();
        // self.denied_commands.clear();
        self.allowed_roles.clear();
        self.denied_roles.clear();
        self.allowed_users.clear();
        self.denied_users.clear();
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
                    denied_users)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *",
            self.default_allow_all_commands,
            self.default_allow_all_users,
            self.default_allow_all_roles,
            // json!(settings.allowed_commands) as serde_json::Value, // Convert to JSON
            // json!(settings.denied_commands) as serde_json::Value,
            &self
                .allowed_roles
                .iter()
                .map(|&x| x as i64)
                .collect::<Vec<i64>>(), // Convert to Vec<i64>
            &self
                .denied_roles
                .iter()
                .map(|&x| x as i64)
                .collect::<Vec<i64>>(),
            &self
                .allowed_users
                .iter()
                .map(|&x| x as i64)
                .collect::<Vec<i64>>(),
            &self
                .denied_users
                .iter()
                .map(|&x| x as i64)
                .collect::<Vec<i64>>(),
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
    pub channel_id: ChannelId,
    pub guild_id: GuildId,
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

impl Default for CommandChannel {
    fn default() -> Self {
        Self {
            command: "".to_string(),
            channel_id: ChannelId::new(0),
            guild_id: GuildId::new(0),
            permission_settings: GenericPermissionSettings::default(),
        }
    }
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
            channel_id: ChannelId::new(read.channel_id as u64),
            guild_id: GuildId::new(read.guild_id as u64),
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
            self.guild_id.get() as i64,
            self.channel_id.get() as i64,
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

pub async fn command_check_music(ctx: Context<'_>) -> Result<bool, Error> {
    if ctx.author().bot {
        return Ok(false);
    };

    // let data: &Data = ctx.data();
    // let user_row = data.userinfo_db.get(ctx.author().id.into()).await?;
    // if user_row.bot_banned() {
    //     notify_banned(ctx).await?;
    //     return Ok(false);
    // }

    let Some(guild_id) = ctx.guild_id() else {
        return Ok(true);
    };

    Ok(ctx
        .data()
        .check_music_permissions(guild_id, ctx.author().id)
        .await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::PgPool;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[ctor::ctor]
    fn set_env() {
        use std::env;
        if env::var("DATABASE_URL").is_err() {
            // env::set_var("DATABASE_URL", "postgresql://localhost:5432/postgres");
            println!("WARNING: DATABASE_URL not set for tests");
        }
    }

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
        settings.add_allowed_role(1);
        assert!(settings.is_role_allowed(1));
        assert!(!settings.is_role_allowed(2));
        settings.add_denied_role(1);
        assert!(!settings.is_role_allowed(1));
    }

    #[test]
    fn test_is_user_allowed() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_user(1);
        assert!(settings.is_user_allowed(1));
        assert!(!settings.is_user_allowed(2));
        settings.add_denied_user(1);
        assert!(!settings.is_user_allowed(1));
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
        settings.add_allowed_role(1);
        assert!(settings.is_role_allowed(1));
        settings.remove_allowed_role(1);
        assert!(settings.is_role_allowed(1));
        settings.add_allowed_role(2);
        assert!(!settings.is_role_allowed(1));
    }

    #[test]
    fn test_add_remove_denied_role() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_denied_role(1);
        assert!(!settings.is_role_allowed(1));
        settings.remove_denied_role(1);
        assert!(settings.is_role_allowed(1));
        settings.add_denied_role(2);
        assert!(settings.is_role_allowed(1));
    }

    #[test]
    fn test_add_remove_allowed_user() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_user(1);
        assert!(settings.is_user_allowed(1));
        settings.remove_allowed_user(1);
        assert!(settings.is_user_allowed(1));
    }

    #[test]
    fn test_add_remove_denied_user() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_denied_user(1);
        assert!(!settings.is_user_allowed(1));
        settings.remove_denied_user(1);
        assert!(settings.is_user_allowed(1));
    }

    #[test]
    fn test_clear() {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test".to_string());
        settings.add_allowed_role(1);
        settings.add_denied_role(1);
        settings.add_allowed_user(1);
        settings.add_denied_user(1);
        settings.clear();
        // assert!(settings.is_command_allowed("test"));
        assert!(settings.is_role_allowed(1));
        assert!(settings.is_user_allowed(1));
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
        };
        let settings = settings_read.convert();
        // assert!(settings.is_command_allowed("test"));
        assert!(!settings.is_role_allowed(1));
        assert!(!settings.is_user_allowed(1));
        assert!(settings.is_role_allowed(2));
        assert!(settings.is_user_allowed(2));
    }

    #[test]
    fn test_convert_to_hash_set_string() {
        let value = json!(["test", "test2"]);
        let hash_set: HashSet<String> = ConvertToHashSetString::convert(value);
        assert!(hash_set.contains("test"));
        assert!(hash_set.contains("test2"));
    }

    #[test]
    fn test_convert_to_hash_set_u64() {
        let value = json!([1, 2]);
        let hash_set = ConvertToHashSetU64::convert(value);
        assert!(hash_set.contains(&1));
        assert!(hash_set.contains(&2));

        let value2 = vec![1, 2];
        let hash_set2 = value2.convert();
        assert!(hash_set2.contains(&1));
        assert!(hash_set2.contains(&2));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_permission_settings(pool: PgPool) {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test2".to_string());
        settings.add_allowed_role(1);
        settings.add_allowed_user(1);
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

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_command_channel(pool: PgPool) {
        let mut settings = GenericPermissionSettings::default();
        // settings.add_allowed_command("test".to_string());
        // settings.add_denied_command("test2".to_string());
        settings.add_allowed_role(1);
        settings.add_allowed_user(1);
        let channel = CommandChannel {
            permission_settings: settings,
            channel_id: ChannelId::new(1),
            guild_id: GuildId::new(1),
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
