//! Opaque, platform-tagged, kind-typed references.
//!
//! - **Kind-typed**: a `RoomRef` cannot be passed where a `CommunityRef` is expected.
//! - **Platform-tagged**: Discord user `123` and another platform's user `123` differ.
//! - **Opaque**: business logic compares, hashes, stores and prints refs; only a
//!   platform adapter knows what the raw text means.
//!
//! Storage is a `(platform, raw)` pair; JSON is `{"platform": "...", "id": "..."}`;
//! a one-line key, `<platform>:<raw>`, serves logs, config and map keys.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The platforms this model has adapters for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Platform {
    Discord,
}

impl Platform {
    const ALL: [Platform; 1] = [Platform::Discord];

    /// The value in the `platform` column, and the prefix of a key.
    pub const fn as_str(self) -> &'static str {
        match self {
            Platform::Discord => "discord",
        }
    }
}

impl FromStr for Platform {
    type Err = RefError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Platform::ALL
            .into_iter()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| RefError::UnknownPlatform(s.to_string()))
    }
}

/// What a reference points at. Sealed: the set of kinds is part of the model.
pub trait Kind: sealed::Sealed + 'static {
    const NAME: &'static str;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! kinds {
    ($($(#[$doc:meta])* $kind:ident => $alias:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $kind {}
        impl sealed::Sealed for $kind {}
        impl Kind for $kind {
            const NAME: &'static str = stringify!($kind);
        }
        pub type $alias = Ref<$kind>;
    )*};
}

kinds! {
    /// A person or bot account.
    User => UserRef,
    /// A server, guild or space.
    Community => CommunityRef,
    /// Where messages (and maybe voice) happen: a channel.
    Room => RoomRef,
    /// One message.
    Message => MessageRef,
    /// A role.
    Role => RoleRef,
}

/// The raw identifier as the platform spells it. Opaque to business logic.
/// Cloning is a reference-count bump.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawId(Arc<str>);

impl RawId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A kind-typed, platform-tagged, opaque reference.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref<K: Kind> {
    platform: Platform,
    raw: RawId,
    kind: PhantomData<K>,
}

impl<K: Kind> Ref<K> {
    /// A ref from a platform id. Empty raw ids and control characters are
    /// refused, so the pair and the key always round-trip.
    pub fn new(platform: Platform, raw: impl AsRef<str>) -> Result<Self, RefError> {
        let raw = raw.as_ref();
        if raw.is_empty() || raw.chars().any(char::is_control) {
            return Err(RefError::BadRaw {
                kind: K::NAME,
                raw: raw.escape_debug().to_string(),
            });
        }
        Ok(Ref {
            platform,
            raw: RawId(Arc::from(raw)),
            kind: PhantomData,
        })
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// The raw text, for adapters converting back to a platform type.
    pub fn raw(&self) -> &RawId {
        &self.raw
    }

    /// The raw text, after checking the ref belongs to `platform`.
    pub fn raw_for(&self, platform: Platform) -> Result<&str, RefError> {
        if self.platform == platform {
            Ok(self.raw.as_str())
        } else {
            Err(RefError::WrongPlatform {
                kind: K::NAME,
                expected: platform,
                found: self.platform,
            })
        }
    }

    /// The one-line key, `<platform>:<raw>`, for logs and config files.
    pub fn key(&self) -> String {
        format!("{}:{}", self.platform.as_str(), self.raw.as_str())
    }
}

impl<K: Kind> fmt::Debug for Ref<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", K::NAME, self.key())
    }
}
impl<K: Kind> fmt::Display for Ref<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

impl<K: Kind> FromStr for Ref<K> {
    type Err = RefError;

    fn from_str(key: &str) -> Result<Self, Self::Err> {
        let (platform, raw) = key.split_once(':').ok_or_else(|| RefError::BadKey {
            kind: K::NAME,
            key: key.to_string(),
        })?;
        Ref::new(platform.parse()?, raw)
    }
}

/// The JSON shape of a ref: the column pair, with the platform typed.
#[derive(Serialize, Deserialize)]
struct RefRepr<'a> {
    platform: Platform,
    #[serde(borrow)]
    id: std::borrow::Cow<'a, str>,
}

impl<K: Kind> Serialize for Ref<K> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RefRepr {
            platform: self.platform,
            id: std::borrow::Cow::Borrowed(self.raw.as_str()),
        }
        .serialize(s)
    }
}
impl<'de, K: Kind> Deserialize<'de> for Ref<K> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RefRepr::deserialize(d)?;
        Ref::new(r.platform, r.id).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefError {
    #[error("{kind} ref has an unusable raw id {raw:?}")]
    BadRaw { kind: &'static str, raw: String },
    #[error("{kind} key {key:?} is not <platform>:<raw>")]
    BadKey { kind: &'static str, key: String },
    #[error("unknown platform {0:?}")]
    UnknownPlatform(String),
    #[error("{kind} ref belongs to {found:?}, not {expected:?}")]
    WrongPlatform {
        kind: &'static str,
        expected: Platform,
        found: Platform,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ref_round_trips_through_its_key() {
        let r: UserRef = "discord:42".parse().unwrap();
        assert_eq!(
            (r.platform(), r.raw().as_str(), r.key()),
            (Platform::Discord, "42", "discord:42".to_string())
        );
    }
    #[test]
    fn serde_is_the_platform_id_pair() {
        let r = UserRef::new(Platform::Discord, "42").unwrap();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"platform":"discord","id":"42"}"#);
        assert_eq!(serde_json::from_str::<UserRef>(&json).unwrap(), r);
    }
    #[test]
    fn an_empty_or_control_raw_is_refused() {
        assert!(matches!(
            UserRef::new(Platform::Discord, ""),
            Err(RefError::BadRaw { kind: "User", .. })
        ));
        assert!(matches!(
            UserRef::new(Platform::Discord, "4\n2"),
            Err(RefError::BadRaw { .. })
        ));
    }
    #[test]
    fn a_bad_key_is_refused() {
        assert!(matches!(
            "42".parse::<UserRef>(),
            Err(RefError::BadKey { kind: "User", .. })
        ));
        assert_eq!(
            "slack:42".parse::<UserRef>().unwrap_err(),
            RefError::UnknownPlatform("slack".into())
        );
        assert!(matches!(
            "discord:".parse::<UserRef>(),
            Err(RefError::BadRaw { .. })
        ));
    }
    #[test]
    fn deserialising_a_bad_ref_fails() {
        assert!(serde_json::from_str::<UserRef>(r#"{"platform":"discord","id":""}"#).is_err());
        assert!(serde_json::from_str::<UserRef>(r#"{"platform":"slack","id":"1"}"#).is_err());
    }
    #[test]
    fn raw_for_returns_the_raw_id_on_its_platform() {
        assert_eq!(
            RoomRef::new(Platform::Discord, "7")
                .unwrap()
                .raw_for(Platform::Discord),
            Ok("7")
        );
    }
    #[test]
    fn debug_names_the_kind_and_display_is_the_key() {
        let r = CommunityRef::new(Platform::Discord, "5").unwrap();
        assert_eq!(
            (format!("{r:?}"), r.to_string()),
            ("Community(discord:5)".into(), "discord:5".into())
        );
    }
    #[test]
    fn refs_order_by_raw_text_and_dedupe_when_equal() {
        let set: std::collections::BTreeSet<UserRef> =
            ["discord:2", "discord:10", "discord:1", "discord:2"]
                .iter()
                .map(|k| k.parse().unwrap())
                .collect();
        let keys: Vec<String> = set.iter().map(Ref::key).collect();
        assert_eq!(keys, ["discord:1", "discord:10", "discord:2"]);
    }
}
