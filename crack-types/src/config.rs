use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, fs, io::Write, path::Path};

use crate::messaging::message::CrackedMessage;
use crate::reply_handle::MessageOrReplyHandle;
use crate::settings::GuildSettings;
use crate::settings::{
    DEFAULT_DB_URL, DEFAULT_LOG_PREFIX, DEFAULT_PREFIX, DEFAULT_VIDEO_STATUS_POLL_INTERVAL,
    DEFAULT_VOLUME_LEVEL,
};
use crate::CrackedError;

/// Struct for the cammed down kicking configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CamKickConfig {
    pub timeout: u64,
    pub guild_id: u64,
    pub chan_id: u64,
    pub dc_msg: String,
    pub msg_on_deafen: bool,
    pub msg_on_mute: bool,
    pub msg_on_dc: bool,
}

/// Default for the CamKickConfig.
impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            timeout: 0,
            guild_id: 0,
            chan_id: 0,
            // FIXME: This should be a const or a static
            dc_msg: "You have been violated for being cammed down for too long.".to_string(),
            msg_on_deafen: false,
            msg_on_mute: false,
            msg_on_dc: false,
        }
    }
}

/// Display impl for CamKickConfig
impl Display for CamKickConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("timeout:       {:?}\n", self.timeout));
        result.push_str(&format!("guild_id:      {:?}\n", self.guild_id));
        result.push_str(&format!("chan_id:       {:?}\n", self.chan_id));
        result.push_str(&format!("dc_msg:        {:?}\n", self.dc_msg));
        result.push_str(&format!("msg_on_deafen: {}\n", self.msg_on_deafen));
        result.push_str(&format!("msg_on_mute:   {}\n", self.msg_on_mute));
        result.push_str(&format!("msg_on_dc:     {}\n", self.msg_on_dc));

        write!(f, "{}", result)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotCredentials {
    pub discord_token: String,
    pub discord_app_id: String,
    pub spotify_client_id: Option<String>,
    pub spotify_client_secret: Option<String>,
    pub openai_api_key: Option<String>,
    pub virustotal_api_key: Option<String>,
}

impl Default for BotCredentials {
    fn default() -> Self {
        Self {
            discord_token: "XXXX".to_string(),
            discord_app_id: "XXXX".to_string(),
            spotify_client_id: None,
            spotify_client_secret: None,
            openai_api_key: None,
            virustotal_api_key: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotConfig {
    pub video_status_poll_interval: Option<u64>,
    // TODO: Get rid of this, it's redundent with the owners in the serenity library.
    pub owners: Option<Vec<u64>>,
    // Cammed down kicking config
    pub cam_kick: Option<Vec<CamKickConfig>>,
    pub sys_log_channel_id: Option<u64>,
    pub self_deafen: Option<bool>,
    pub volume: Option<f32>,
    #[serde(skip)]
    pub guild_settings_map: Option<Vec<GuildSettings>>,
    pub prefix: Option<String>,
    pub credentials: Option<BotCredentials>,
    pub database_url: Option<String>,
    pub log_prefix: Option<String>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: Some(DEFAULT_VIDEO_STATUS_POLL_INTERVAL),
            owners: None,
            cam_kick: None,
            sys_log_channel_id: None,
            self_deafen: Some(true),
            volume: Some(DEFAULT_VOLUME_LEVEL),
            guild_settings_map: None,
            prefix: Some(DEFAULT_PREFIX.to_string()),
            credentials: Some(BotCredentials::default()),
            database_url: Some(DEFAULT_DB_URL.to_string()),
            log_prefix: Some(DEFAULT_LOG_PREFIX.to_string()),
        }
    }
}

impl Display for BotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!(
            "video_status_poll_interval: {:?}\n",
            self.video_status_poll_interval
        ));
        result.push_str(&format!("owners: {:?}\n", self.owners));
        result.push_str(&format!("cam_kick: {:?}\n", self.cam_kick));
        result.push_str(&format!(
            "sys_log_channel_id: {:?}\n",
            self.sys_log_channel_id
        ));
        result.push_str(&format!("self_deafen: {:?}\n", self.self_deafen));
        result.push_str(&format!("volume: {:?}\n", self.volume));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!(
            "prefix: {}",
            self.prefix
                .as_ref()
                .cloned()
                .unwrap_or(DEFAULT_PREFIX.to_string())
        ));
        result.push_str(&format!("credentials: {:?}\n", self.credentials.is_some()));
        result.push_str(&format!("database_url: {:?}\n", self.database_url));
        result.push_str(&format!("log_prefix: {:?}\n", self.log_prefix));
        write!(f, "{}", result)
    }
}

impl BotConfig {
    pub fn set_credentials(&mut self, creds: BotCredentials) -> &mut Self {
        self.credentials = Some(creds);
        self
    }

    pub fn get_prefix(&self) -> String {
        self.prefix.clone().unwrap_or(DEFAULT_PREFIX.to_string())
    }

    pub fn get_video_status_poll_interval(&self) -> u64 {
        self.video_status_poll_interval
            .unwrap_or(DEFAULT_VIDEO_STATUS_POLL_INTERVAL)
    }

    pub fn get_database_url(&self) -> String {
        self.database_url
            .as_ref()
            .cloned()
            .unwrap_or(DEFAULT_DB_URL.to_string())
    }
}

/// Phone code data for the osint commands
#[derive(Default, Debug, Clone)]
pub struct PhoneCodeData {
    #[allow(dead_code)]
    phone_codes: HashMap<String, String>,
    #[allow(dead_code)]
    country_names: HashMap<String, String>,
    country_by_phone_code: HashMap<String, Vec<String>>,
}

/// impl of PhoneCodeData
impl PhoneCodeData {
    /// Load the phone code data from the local file, or download it if it doesn't exist
    pub fn load() -> Result<Self, CrackedError> {
        let phone_codes = Self::load_data("./data/phone.json", "http://country.io/phone.json")?;
        let country_names = Self::load_data("./data/names.json", "http://country.io/names.json")?;
        let country_by_phone_code = phone_codes
            .iter()
            .map(|(k, v)| (v.clone(), k.clone()))
            .fold(
                HashMap::new(),
                |mut acc: HashMap<String, Vec<String>>, (k, v)| {
                    acc.entry(k).or_default().push(v);
                    acc
                },
            );
        Ok(Self {
            phone_codes,
            country_names,
            country_by_phone_code,
        })
    }

    /// Load the data from the local file, or download it if it doesn't exist
    fn load_data(file_name: &str, url: &str) -> Result<HashMap<String, String>, CrackedError> {
        match fs::read_to_string(file_name) {
            Ok(contents) => serde_json::from_str(&contents).map_err(CrackedError::Json),
            Err(_) => Self::download_and_parse(url, file_name),
        }
    }

    /// Download the data from the url and parse it. Internally used.
    fn download_and_parse(
        url: &str,
        file_name: &str,
    ) -> Result<HashMap<String, String>, CrackedError> {
        let client = reqwest::blocking::ClientBuilder::new()
            .use_rustls_tls()
            .cookie_store(true)
            .build()?;
        //let client = crate::http_utils::get_client();
        let response = client.get(url).send().map_err(CrackedError::Reqwest)?;
        let content = response.text().map_err(CrackedError::Reqwest)?;

        // Save to local file
        fs::create_dir_all(Path::new(file_name).parent().unwrap()).map_err(CrackedError::IO)?;
        let mut file = fs::File::create(file_name).map_err(CrackedError::IO)?;
        file.write_all(content.as_bytes())
            .map_err(CrackedError::IO)?;

        serde_json::from_str(&content).map_err(CrackedError::Json)
    }

    /// Get names of countries that match the given phone code.
    /// Due to edge cases, there may be multiples.
    pub fn get_countries_by_phone_code(&self, phone_code: &str) -> Option<Vec<String>> {
        self.country_by_phone_code.get(phone_code).cloned()
    }
}

#[cfg(test)]
mod test {
    use super::PhoneCodeData;

    #[test]
    fn test_phone_code_data() {
        let data = PhoneCodeData::load().unwrap();
        let country_names = data.country_names;
        let phone_codes = data.phone_codes;
        let country_by_phone_code = data.country_by_phone_code;

        assert_eq!(country_names.get("US"), Some(&"United States".to_string()));
        assert_eq!(phone_codes.get("IS"), Some(&"354".to_string()));
        let want = &vec!["CA".to_string(), "UM".to_string(), "US".to_string()];
        let got = country_by_phone_code.get("1").unwrap();
        // This would be cheaper using a heap or tree
        assert!(got.iter().all(|x| want.contains(x)));
        assert!(want.iter().all(|x| got.contains(x)));
    }
}
