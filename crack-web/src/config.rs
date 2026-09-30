//! The dashboard's environment. Missing keys switch the dashboard off; they
//! never stop the bot.

/// Where the dashboard listens unless `WEB_BIND` says otherwise. Not 8080:
/// the edge proxy already sends the bots VM's 8080 to crack-voting.
pub const DEFAULT_BIND: &str = "0.0.0.0:8090";

/// The dashboard's configuration, read from the environment.
#[derive(Clone)]
pub struct WebEnv {
    pub client_id: String,
    pub client_secret: String,
    /// e.g. `https://dash.cracktun.es`, no trailing slash. Builds the OAuth
    /// redirect and is the only `Origin` a move is accepted from.
    pub public_origin: String,
    pub jwt_secret: String,
    pub bot_token: String,
    pub bind: String,
}

// Hand-written: the derived Debug would print the secrets.
impl std::fmt::Debug for WebEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebEnv")
            .field("client_id", &self.client_id)
            .field("public_origin", &self.public_origin)
            .field("bind", &self.bind)
            .finish_non_exhaustive()
    }
}

impl WebEnv {
    /// Read the configuration through `get`. On failure, returns the name of
    /// every missing or unusable key -- names only, never values.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, Vec<&'static str>> {
        let val = |k: &str| {
            get(k)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let mut missing = Vec::new();

        let client_id = val("DISCORD_CLIENT_ID").or_else(|| val("DISCORD_APP_ID"));
        if client_id.is_none() {
            missing.push("DISCORD_CLIENT_ID");
        }
        let client_secret = val("DISCORD_CLIENT_SECRET");
        if client_secret.is_none() {
            missing.push("DISCORD_CLIENT_SECRET");
        }
        let public_origin = val("WEB_PUBLIC_ORIGIN").map(|o| o.trim_end_matches('/').to_owned());
        match &public_origin {
            None => missing.push("WEB_PUBLIC_ORIGIN"),
            Some(o)
                if !(o.starts_with("https://")
                    || o.starts_with("http://localhost")
                    || o.starts_with("http://127.0.0.1")) =>
            {
                missing.push("WEB_PUBLIC_ORIGIN (https://, or http://localhost)")
            },
            Some(_) => {},
        }
        let jwt_secret = val("WEB_JWT_SECRET");
        match &jwt_secret {
            None => missing.push("WEB_JWT_SECRET"),
            Some(s) if s.len() < 32 => missing.push("WEB_JWT_SECRET (32+ characters)"),
            Some(_) => {},
        }
        let bot_token = val("DISCORD_TOKEN");
        if bot_token.is_none() {
            missing.push("DISCORD_TOKEN");
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        Ok(Self {
            client_id: client_id.unwrap(),
            client_secret: client_secret.unwrap(),
            public_origin: public_origin.unwrap(),
            jwt_secret: jwt_secret.unwrap(),
            bot_token: bot_token.unwrap(),
            bind: val("WEB_BIND").unwrap_or_else(|| DEFAULT_BIND.to_owned()),
        })
    }

    /// The OAuth redirect: catacombs' callback, mounted at `/auth`.
    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_origin)
    }

    /// catacombs' configuration. Built here rather than by
    /// `catacombs::Config::from_env`, whose variable names differ from ours.
    pub fn catacombs_config(&self) -> catacombs::Config {
        catacombs::Config {
            discord: catacombs::DiscordConfig {
                client_id: self.client_id.clone(),
                client_secret: self.client_secret.clone(),
                redirect_uri: self.redirect_uri(),
                bot_token: self.bot_token.clone(),
                premium_sku_id: None,
                api_base: catacombs::config::default_api_base(),
            },
            security: catacombs::SecurityConfig {
                jwt_secret: self.jwt_secret.clone(),
                // Memory storage keeps nothing encrypted; the field is
                // required, so it gets a per-process random value.
                encryption_key: format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                ),
            },
            server: catacombs::ServerConfig::default(),
            web: catacombs::WebConfig::default(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    const SECRET32: &str = "0123456789abcdef0123456789abcdef";

    fn full() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DISCORD_CLIENT_ID", "111".to_string()),
            ("DISCORD_CLIENT_SECRET", "shh".to_string()),
            ("WEB_PUBLIC_ORIGIN", "https://dash.cracktun.es/".to_string()),
            ("WEB_JWT_SECRET", SECRET32.to_string()),
            ("DISCORD_TOKEN", "bot".to_string()),
        ])
    }

    fn load(env: &HashMap<&'static str, String>) -> Result<WebEnv, Vec<&'static str>> {
        WebEnv::from_lookup(|k| env.get(k).cloned())
    }

    #[test]
    fn a_full_environment_loads_with_defaults() {
        let env = load(&full()).unwrap();
        assert_eq!(env.public_origin, "https://dash.cracktun.es");
        assert_eq!(env.redirect_uri(), "https://dash.cracktun.es/auth/callback");
        assert_eq!(env.bind, DEFAULT_BIND);
        let c = env.catacombs_config();
        assert_eq!(c.discord.client_id, "111");
        assert_eq!(
            c.discord.redirect_uri,
            "https://dash.cracktun.es/auth/callback"
        );
        assert_eq!(c.web.scopes, vec!["identify".to_string()]);
        assert!(c.web.secure_cookies);
    }

    #[test]
    fn the_app_id_stands_in_for_a_missing_client_id() {
        let mut e = full();
        e.remove("DISCORD_CLIENT_ID");
        e.insert("DISCORD_APP_ID", "222".to_string());
        assert_eq!(load(&e).unwrap().client_id, "222");
    }

    #[test]
    fn every_missing_key_is_named_and_blank_counts_as_missing() {
        let mut e = full();
        e.remove("DISCORD_CLIENT_SECRET");
        e.insert("WEB_PUBLIC_ORIGIN", "  ".to_string());
        e.remove("DISCORD_CLIENT_ID");
        let missing = load(&e).unwrap_err();
        assert_eq!(
            missing,
            vec![
                "DISCORD_CLIENT_ID",
                "DISCORD_CLIENT_SECRET",
                "WEB_PUBLIC_ORIGIN"
            ]
        );
    }

    #[test]
    fn a_short_jwt_secret_is_refused() {
        let mut e = full();
        e.insert("WEB_JWT_SECRET", "short".to_string());
        assert_eq!(
            load(&e).unwrap_err(),
            vec!["WEB_JWT_SECRET (32+ characters)"]
        );
    }

    #[test]
    fn a_plain_http_origin_is_refused_except_localhost() {
        let mut e = full();
        e.insert("WEB_PUBLIC_ORIGIN", "http://dash.cracktun.es".to_string());
        assert_eq!(
            load(&e).unwrap_err(),
            vec!["WEB_PUBLIC_ORIGIN (https://, or http://localhost)"]
        );
        e.insert("WEB_PUBLIC_ORIGIN", "http://localhost:8090".to_string());
        assert!(load(&e).is_ok());
    }

    #[test]
    fn web_bind_overrides_the_default() {
        let mut e = full();
        e.insert("WEB_BIND", "127.0.0.1:9000".to_string());
        assert_eq!(load(&e).unwrap().bind, "127.0.0.1:9000");
    }
}
