use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Struct for generic permission settings. Includes allowed and denied commands, roles, and users.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenericPermissionSettings {
    #[serde(default = "default_true")]
    pub default_allow_all_commands: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_users: bool,
    #[serde(default = "default_true")]
    pub default_allow_all_roles: bool,
    pub allowed_commands: HashSet<String>,
    pub denied_commands: HashSet<String>,
    pub allowed_roles: HashSet<u64>,
    pub denied_roles: HashSet<u64>,
    pub allowed_users: HashSet<u64>,
    pub denied_users: HashSet<u64>,
}

/// Default true for serialization
fn default_true() -> bool {
    true
}

/// Default implementation for GenericPermissionSettings.
impl Default for GenericPermissionSettings {
    fn default() -> Self {
        Self {
            default_allow_all_commands: true,
            default_allow_all_users: true,
            default_allow_all_roles: true,
            allowed_commands: HashSet::new(),
            denied_commands: HashSet::new(),
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
    pub fn is_command_allowed(&self, command: &str) -> bool {
        (self.allowed_commands.is_empty()
            && self.denied_commands.is_empty()
            && self.default_allow_all_commands)
            || self.allowed_commands.is_empty()
                && self.default_allow_all_commands
                && !self.denied_commands.contains(command)
            || self.allowed_commands.contains(command) && !self.denied_commands.contains(command)
    }

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

    /// Add a command to the allowed commands.
    pub fn add_allowed_command(&mut self, command: String) {
        self.allowed_commands.insert(command);
    }

    /// Remove a command from the allowed commands.
    pub fn remove_allowed_command(&mut self, command: &str) {
        self.allowed_commands.remove(command);
    }

    /// Add a command to the denied commands.
    pub fn add_denied_command(&mut self, command: String) {
        self.denied_commands.insert(command);
    }

    /// Remove a command from the denied commands.
    pub fn remove_denied_command(&mut self, command: &str) {
        self.denied_commands.remove(command);
    }

    /// Add a role to the allowed roles.
    pub fn add_allowed_role(&mut self, role: u64) {
        self.allowed_roles.insert(role);
    }

    /// Remove a role from the allowed roles.
    pub fn remove_allowed_role(&mut self, role: u64) {
        self.allowed_roles.remove(&role);
    }

    /// Add a role to the denied roles.
    pub fn add_denied_role(&mut self, role: u64) {
        self.denied_roles.insert(role);
    }

    /// Remove a role from the denied roles.
    pub fn remove_denied_role(&mut self, role: u64) {
        self.denied_roles.remove(&role);
    }

    /// Add a user to the allowed users.
    pub fn add_allowed_user(&mut self, user: u64) {
        self.allowed_users.insert(user);
    }

    /// Remove a user from the allowed users.
    pub fn remove_allowed_user(&mut self, user: u64) {
        self.allowed_users.remove(&user);
    }

    /// Add a user to the denied users.
    pub fn add_denied_user(&mut self, user: u64) {
        self.denied_users.insert(user);
    }

    /// Remove a user from the denied users.
    pub fn remove_denied_user(&mut self, user: u64) {
        self.denied_users.remove(&user);
    }

    /// Clear all allowed and denied commands, roles, and users.
    pub fn clear(&mut self) {
        self.allowed_commands.clear();
        self.denied_commands.clear();
        self.allowed_roles.clear();
        self.denied_roles.clear();
        self.allowed_users.clear();
        self.denied_users.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_command_allowed() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_command("test".to_string());
        assert!(settings.is_command_allowed("test"));
        assert!(!settings.is_command_allowed("test2"));
        settings.add_denied_command("test".to_string());
        assert!(!settings.is_command_allowed("test"));
        assert!(!settings.is_command_allowed("test2"));
    }

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

    #[test]
    fn test_add_remove_allowed_command() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_allowed_command("test".to_string());
        assert!(settings.is_command_allowed("test"));
        settings.remove_allowed_command("test");
        assert!(settings.is_command_allowed("test"));
        settings.add_allowed_command("test2".to_string());
        assert!(!settings.is_command_allowed("test"));
    }

    #[test]
    fn test_add_remove_denied_command() {
        let mut settings = GenericPermissionSettings::default();
        settings.add_denied_command("test".to_string());
        assert!(!settings.is_command_allowed("test"));
        settings.remove_denied_command("test");
        assert!(settings.is_command_allowed("test"));
    }

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
}
