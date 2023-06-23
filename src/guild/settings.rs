use self::serenity::model::prelude::UserId;
use self::serenity::{model::id::GuildId, TypeMapKey};
use lazy_static::lazy_static;
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{create_dir_all, OpenOptions},
    io::{BufReader, BufWriter},
    path::Path,
};

use crate::errors::CrackedError;

const DEFAULT_SETTINGS_PATH: &str = "data/settings";
const DEFAULT_ALLOWED_DOMAINS: [&str; 1] = ["youtube.com"];
const DEFAULT_VOLUME_LEVEL: f32 = 0.2;

lazy_static! {
    static ref SETTINGS_PATH: String =
        env::var("SETTINGS_PATH").unwrap_or(DEFAULT_SETTINGS_PATH.to_string());
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct GuildSettings {
    pub guild_id: GuildId,
    pub autopause: bool,
    pub allowed_domains: HashSet<String>,
    pub banned_domains: HashSet<String>,
    pub authorized_users: HashSet<u64>,
    pub volume: f32,
}

impl GuildSettings {
    pub fn new(guild_id: GuildId) -> GuildSettings {
        let allowed_domains: HashSet<String> = DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|d| d.to_string())
            .collect();

        GuildSettings {
            guild_id,
            autopause: false,
            allowed_domains,
            banned_domains: HashSet::new(),
            authorized_users: HashSet::new(),
            volume: DEFAULT_VOLUME_LEVEL,
        }
    }

    pub fn load_if_exists(&mut self) -> Result<(), CrackedError> {
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);
        if !Path::new(&path).exists() {
            return Ok(());
        }
        self.load()
    }

    pub fn load(&mut self) -> Result<(), CrackedError> {
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);
        let file = OpenOptions::new().read(true).open(path)?;
        let reader = BufReader::new(file);
        *self = serde_json::from_reader::<_, GuildSettings>(reader)?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), CrackedError> {
        create_dir_all(SETTINGS_PATH.as_str())?;
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);

        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)?;

        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, self)?;
        Ok(())
    }

    pub fn toggle_autopause(&mut self) {
        self.autopause = !self.autopause;
    }

    pub fn set_allowed_domains(&mut self, allowed_str: &str) {
        let allowed = allowed_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.allowed_domains = allowed;
    }

    pub fn set_banned_domains(&mut self, banned_str: &str) {
        let banned = banned_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.banned_domains = banned;
    }

    pub fn update_domains(&mut self) {
        if !self.allowed_domains.is_empty() && !self.banned_domains.is_empty() {
            self.banned_domains.clear();
        }

        if self.allowed_domains.is_empty() && self.banned_domains.is_empty() {
            self.allowed_domains.insert(String::from("youtube.com"));
            self.banned_domains.clear();
        }
    }

    pub fn authorize_user(&mut self, user_id: u64) {
        if !self.authorized_users.contains(&user_id) {
            self.authorized_users.insert(user_id);
        }
    }

    pub fn deauthorize_user(&mut self, user_id: u64) {
        if self.authorized_users.contains(&user_id) {
            self.authorized_users.remove(&user_id);
        }
    }

    pub fn check_authorized(&self, user_id: u64) -> bool {
        self.authorized_users.contains(&user_id)
    }

    pub fn check_authorized_user_id(&self, user_id: UserId) -> bool {
        self.authorized_users.contains(&user_id.0)
    }

    pub fn set_default_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
}

pub struct GuildSettingsMap;

impl TypeMapKey for GuildSettingsMap {
    type Value = HashMap<GuildId, GuildSettings>;
}

pub struct Float;

impl TypeMapKey for Float {
    type Value = f32;
}
