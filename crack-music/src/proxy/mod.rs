//! Residential-proxy provider seam.
//!
//! Secrets (password, authenticated URL, user with sessid) must never be
//! logged or printed un-redacted.

pub mod gate;

use once_cell::sync::Lazy;

/// Kind of proxy a provider offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyKind {
    Residential,
}

/// A password that never prints itself.
#[derive(Clone)]
pub struct ProxyPassword(String);

impl ProxyPassword {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ProxyPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

impl std::fmt::Display for ProxyPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

/// One sticky proxy session (one exit IP).
pub struct ProxySession {
    /// Authenticated URL for yt-dlp. Secret: never log.
    pub proxy_url: String,
    user: String,
    password: ProxyPassword,
}

impl std::fmt::Debug for ProxySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxySession").finish_non_exhaustive()
    }
}

impl ProxySession {
    /// Proxy for reqwest; credentials are passed via basic auth, not the URL.
    pub fn reqwest_proxy(&self) -> reqwest::Result<reqwest::Proxy> {
        Ok(reqwest::Proxy::all(self.base_url())?.basic_auth(&self.user, self.password.expose()))
    }

    /// Authenticated URL for yt-dlp's `--proxy`.
    pub fn ytdl_proxy_url(&self) -> &str {
        &self.proxy_url
    }

    fn base_url(&self) -> String {
        // proxy_url is "http://user:pw@host:port"; strip the userinfo.
        let rest = self
            .proxy_url
            .strip_prefix("http://")
            .unwrap_or(&self.proxy_url);
        let host = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
        format!("http://{host}")
    }
}

pub trait ProxyProvider: Send + Sync {
    fn kind(&self) -> ProxyKind;
    fn session(&self) -> ProxySession;
}

/// DataImpulse residential gateway.
#[derive(Debug)]
pub struct DataImpulseResidential {
    host: String,
    port: u16,
    base_user: String,
    password: ProxyPassword,
}

impl ProxyProvider for DataImpulseResidential {
    fn kind(&self) -> ProxyKind {
        ProxyKind::Residential
    }

    fn session(&self) -> ProxySession {
        let sessid = format!("{:016x}", rand::random::<u64>());
        let user = format!("{};sessid-{sessid}", self.base_user);
        let proxy_url = format!(
            "http://{}:{}@{}:{}",
            encode_userinfo(&user),
            encode_userinfo(self.password.expose()),
            self.host,
            self.port
        );
        ProxySession {
            proxy_url,
            user,
            password: self.password.clone(),
        }
    }
}

/// Percent-encode everything except RFC 3986 unreserved bytes.
fn encode_userinfo(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn from_env_from(get: impl Fn(&str) -> Option<String>) -> Option<Box<dyn ProxyProvider>> {
    let host = get("PROXY_FALLBACK_HOST").filter(|s| !s.is_empty())?;
    let base_user = get("PROXY_FALLBACK_USER").filter(|s| !s.is_empty())?;
    let password = ProxyPassword::new(get("PROXY_FALLBACK_PASSWORD").filter(|s| !s.is_empty())?);
    let port = get("PROXY_FALLBACK_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(823);
    Some(Box::new(DataImpulseResidential {
        host,
        port,
        base_user,
        password,
    }))
}

/// `Some` iff HOST, USER and PASSWORD are all set (port defaults to 823).
pub fn from_env() -> Option<Box<dyn ProxyProvider>> {
    from_env_from(|k| std::env::var(k).ok())
}

static PROVIDER: Lazy<Option<Box<dyn ProxyProvider>>> = Lazy::new(from_env);

/// Process-wide provider configured from the environment.
pub fn provider() -> Option<&'static dyn ProxyProvider> {
    PROVIDER.as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn di_provider_with(p: ProxyPassword) -> DataImpulseResidential {
        DataImpulseResidential {
            host: "gw.example.test".into(),
            port: 823,
            base_user: "baseuser".into(),
            password: p,
        }
    }

    fn di() -> DataImpulseResidential {
        di_provider_with(ProxyPassword::new("hunter2"))
    }

    fn sessid_of(url: &str) -> &str {
        let (_, rest) = url.split_once("%3Bsessid-").unwrap();
        rest.split(':').next().unwrap()
    }

    #[test]
    fn session_user_has_fresh_sessid() {
        let p = di();
        let (a, b) = (p.session(), p.session());
        assert!(a.user.starts_with("baseuser;sessid-"));
        assert!(b.user.starts_with("baseuser;sessid-"));
        assert_ne!(a.user, b.user);
    }

    #[test]
    fn ytdl_proxy_url_is_authenticated_and_percent_encodes_semicolon() {
        let s = di().session();
        let u = s.ytdl_proxy_url();
        assert!(u.contains("%3Bsessid-"));
        assert!(!u.contains(';'));
        assert!(u.contains("gw.example.test:823"));
        assert!(u.contains("hunter2"));
        assert_eq!(sessid_of(u).len(), 16);
    }

    #[test]
    fn password_redacts_in_debug() {
        let p = ProxyPassword::new("hunter2");
        assert_eq!(format!("{p:?}"), "***");
        assert_eq!(format!("{p}"), "***");
        let prov = di_provider_with(p);
        assert!(!format!("{prov:?}").contains("hunter2"));
        assert!(!format!("{:?}", prov.session()).contains("hunter2"));
    }

    fn env_of(pairs: &[(&'static str, &str)]) -> HashMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, (*v).to_string())).collect()
    }

    fn none_for(pairs: &[(&'static str, &str)]) -> bool {
        let m = env_of(pairs);
        from_env_from(|k| m.get(k).cloned()).is_none()
    }

    #[test]
    fn from_env_none_without_full_creds() {
        assert!(none_for(&[
            ("PROXY_FALLBACK_USER", "u"),
            ("PROXY_FALLBACK_PASSWORD", "p")
        ]));
        assert!(none_for(&[
            ("PROXY_FALLBACK_HOST", "h"),
            ("PROXY_FALLBACK_PASSWORD", "p")
        ]));
        assert!(none_for(&[
            ("PROXY_FALLBACK_HOST", "h"),
            ("PROXY_FALLBACK_USER", "u")
        ]));
        assert!(none_for(&[]));
    }

    #[test]
    fn from_env_none_when_a_var_is_empty() {
        assert!(none_for(&[
            ("PROXY_FALLBACK_HOST", ""),
            ("PROXY_FALLBACK_USER", "u"),
            ("PROXY_FALLBACK_PASSWORD", "p"),
        ]));
    }

    #[test]
    fn proxy_url_round_trips_special_chars() {
        let raw = "p@ss:w/rd%#?[] 41%41";
        let s = di_provider_with(ProxyPassword::new(raw)).session();
        let u = url::Url::parse(&s.proxy_url).expect("parses");
        let dec = |x: &str| {
            url::form_urlencoded::parse(format!("k={}", x.replace('+', "%2B")).as_bytes())
                .next()
                .unwrap()
                .1
                .into_owned()
        };
        assert_eq!(dec(u.password().unwrap()), raw);
        let user = dec(u.username());
        assert_eq!(user, s.user);
        assert!(user.contains(";sessid-"));
        assert_eq!(u.host_str(), Some("gw.example.test"));
    }

    #[test]
    fn from_env_some_with_full_creds_default_port() {
        let m: HashMap<&str, String> = [
            ("PROXY_FALLBACK_HOST", "h.test".to_string()),
            ("PROXY_FALLBACK_USER", "u".to_string()),
            ("PROXY_FALLBACK_PASSWORD", "hunter2".to_string()),
        ]
        .into();
        let p = from_env_from(|k| m.get(k).cloned()).expect("some");
        assert_eq!(p.kind(), ProxyKind::Residential);
        assert!(p.session().ytdl_proxy_url().contains("h.test:823"));
    }

    #[test]
    fn reqwest_proxy_builds() {
        assert!(di().session().reqwest_proxy().is_ok());
    }
}
