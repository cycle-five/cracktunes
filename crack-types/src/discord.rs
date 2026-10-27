//! Where serenity's id types and platform-neutral refs meet.
//!
//! Everything that turns a stored or configured id into a serenity id goes
//! through here, because serenity's `new` panics on `u64::MAX`, and a snowflake is never `0`.

use crack_model::{Kind, Platform, Ref, RoomRef};
use serenity::all::{ChannelId, GenericChannelId, GuildId, MessageId, RoleId, UserId};

/// Why a ref could not become a Discord id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("{kind} ref belongs to {found:?}, not Discord")]
    WrongPlatform { kind: &'static str, found: Platform },
    #[error("{kind} id {raw:?} is not a Discord snowflake")]
    NotASnowflake { kind: &'static str, raw: String },
}

/// A serenity id that has a ref.
pub trait ToRef {
    type Kind: Kind;
    fn to_ref(&self) -> Ref<Self::Kind>;
}

/// A Discord ref from a snowflake. Zero and `u64::MAX` are refused.
pub fn from_snowflake<K: Kind>(id: u64) -> Result<Ref<K>, IdError> {
    if id == 0 || id == u64::MAX {
        return Err(IdError::NotASnowflake {
            kind: K::NAME,
            raw: id.to_string(),
        });
    }
    Ref::new(Platform::Discord, id.to_string()).map_err(|_| IdError::NotASnowflake {
        kind: K::NAME,
        raw: id.to_string(),
    })
}

/// The snowflake behind a Discord ref: canonical decimal digits (no sign, no
/// leading zero, no whitespace) with `1 <= n < u64::MAX`.
pub fn snowflake<K: Kind>(r: &Ref<K>) -> Result<u64, IdError> {
    let raw = r
        .raw_for(Platform::Discord)
        .map_err(|_| IdError::WrongPlatform {
            kind: K::NAME,
            found: r.platform(),
        })?;
    let not = || IdError::NotASnowflake {
        kind: K::NAME,
        raw: raw.to_string(),
    };
    if raw.starts_with('0') || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(not());
    }
    match raw.parse::<u64>() {
        Ok(n) if n != u64::MAX => Ok(n),
        _ => Err(not()),
    }
}

macro_rules! discord_ids {
    ($($id:ident => $kind:ident, $back:ident;)*) => {$(
        impl ToRef for $id {
            type Kind = crack_model::$kind;
            fn to_ref(&self) -> Ref<Self::Kind> {
                // Decimal text is always a valid raw id. Id 0 (and u64::MAX, which
                // serenity cannot hold) maps to a ref that no back-conversion accepts.
                Ref::new(Platform::Discord, self.get().to_string())
                    .expect("a decimal id is a valid raw id")
            }
        }
        pub fn $back(r: &Ref<crack_model::$kind>) -> Result<$id, IdError> {
            snowflake(r).map($id::new)
        }
    )*};
}

discord_ids! {
    GuildId => Community, guild_id;
    UserId => User, user_id;
    ChannelId => Room, channel_id;
    RoleId => Role, role_id;
    MessageId => Message, message_id;
}

impl ToRef for GenericChannelId {
    type Kind = crack_model::Room;
    fn to_ref(&self) -> Ref<Self::Kind> {
        Ref::new(Platform::Discord, self.get().to_string()).expect("a decimal id is a valid raw id")
    }
}

pub fn generic_channel_id(r: &RoomRef) -> Result<GenericChannelId, IdError> {
    snowflake(r).map(GenericChannelId::new)
}

/// `#[serde(with = "…::as_snowflake")]`: a ref as a bare JSON number.
pub mod as_snowflake {
    use super::{from_snowflake, snowflake, Kind, Ref};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<K: Kind, S: Serializer>(r: &Ref<K>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(snowflake(r).map_err(serde::ser::Error::custom)?)
    }

    pub fn deserialize<'de, K: Kind, D: Deserializer<'de>>(d: D) -> Result<Ref<K>, D::Error> {
        from_snowflake(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// `#[serde(with = "…::as_snowflake_string")]`: a ref as serenity writes an id,
/// a JSON string of the decimal snowflake. Reads a string or a number.
pub mod as_snowflake_string {
    use super::{from_snowflake, snowflake, Kind, Ref};
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;
    use std::marker::PhantomData;

    pub fn serialize<K: Kind, S: Serializer>(r: &Ref<K>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&snowflake(r).map_err(serde::ser::Error::custom)?)
    }

    struct SnowflakeVisitor<K>(PhantomData<K>);

    impl<K: Kind> Visitor<'_> for SnowflakeVisitor<K> {
        type Value = Ref<K>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a snowflake as a string or an integer")
        }

        fn visit_u64<E: de::Error>(self, n: u64) -> Result<Ref<K>, E> {
            from_snowflake(n).map_err(E::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Ref<K>, E> {
            // The same rule as `snowflake`: canonical digits only.
            let canonical = !v.starts_with('0') && v.bytes().all(|b| b.is_ascii_digit());
            match v.parse::<u64>() {
                Ok(n) if canonical => self.visit_u64(n),
                _ => Err(E::custom(format!("{:?} is not a snowflake", v))),
            }
        }
    }

    pub fn deserialize<'de, K: Kind, D: Deserializer<'de>>(d: D) -> Result<Ref<K>, D::Error> {
        d.deserialize_any(SnowflakeVisitor(PhantomData))
    }
}

/// `#[serde(with = "…::as_snowflake_opt", default)]` for `Option<Ref<K>>`.
pub mod as_snowflake_opt {
    use super::{as_snowflake, from_snowflake, Kind, Ref};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<K: Kind, S: Serializer>(r: &Option<Ref<K>>, s: S) -> Result<S::Ok, S::Error> {
        match r {
            Some(r) => as_snowflake::serialize(r, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, K: Kind, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Ref<K>>, D::Error> {
        Option::<u64>::deserialize(d)?
            .map(|n| from_snowflake(n).map_err(serde::de::Error::custom))
            .transpose()
    }
}

/// `#[serde(with = "…::as_snowflake_set")]` for `HashSet<Ref<K>>`.
pub mod as_snowflake_set {
    use super::{from_snowflake, snowflake, Kind, Ref};
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::collections::HashSet;
    use std::hash::Hash;

    pub fn serialize<K: Kind + Hash + Eq, S: Serializer>(
        set: &HashSet<Ref<K>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(set.len()))?;
        for r in set {
            seq.serialize_element(&snowflake(r).map_err(serde::ser::Error::custom)?)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, K: Kind + Hash + Eq, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashSet<Ref<K>>, D::Error> {
        Vec::<u64>::deserialize(d)?
            .into_iter()
            .map(|n| from_snowflake(n).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_model::{CommunityRef, RoleRef, User, UserRef};
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;

    #[test]
    fn a_guild_id_round_trips() {
        let g = GuildId::new(81384788765712384);
        assert_eq!(g.to_ref().key(), "discord:81384788765712384");
        assert_eq!(guild_id(&g.to_ref()), Ok(g));
    }
    #[test]
    fn every_id_kind_round_trips() {
        assert_eq!(user_id(&UserId::new(7).to_ref()), Ok(UserId::new(7)));
        assert_eq!(
            channel_id(&ChannelId::new(8).to_ref()),
            Ok(ChannelId::new(8))
        );
        assert_eq!(
            generic_channel_id(&GenericChannelId::new(9).to_ref()),
            Ok(GenericChannelId::new(9))
        );
        assert_eq!(role_id(&RoleId::new(10).to_ref()), Ok(RoleId::new(10)));
        assert_eq!(
            message_id(&MessageId::new(11).to_ref()),
            Ok(MessageId::new(11))
        );
    }
    #[test]
    fn zero_max_overflow_and_junk_are_not_snowflakes() {
        for raw in [
            "0",
            "18446744073709551615",
            "18446744073709551616",
            "-1",
            "+1",
            "12a",
            "007",
            " 1",
        ] {
            let r = UserRef::new(Platform::Discord, raw).unwrap();
            assert_eq!(
                snowflake(&r),
                Err(IdError::NotASnowflake {
                    kind: "User",
                    raw: raw.into()
                }),
                "{raw}"
            );
            assert!(user_id(&r).is_err(), "{raw}");
        }
        assert!(from_snowflake::<User>(0).is_err());
        assert!(from_snowflake::<User>(u64::MAX).is_err());
        assert_eq!(from_snowflake::<User>(42).unwrap().key(), "discord:42");
    }
    #[test]
    fn serde_adapters_keep_bare_numbers() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct S {
            #[serde(with = "as_snowflake")]
            g: CommunityRef,
            #[serde(with = "as_snowflake_opt", default)]
            c: Option<RoomRef>,
            #[serde(with = "as_snowflake_set")]
            s: HashSet<RoleRef>,
        }
        let s = S {
            g: GuildId::new(5).to_ref(),
            c: None,
            s: [RoleId::new(7).to_ref()].into(),
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"g":5,"c":null,"s":[7]}"#
        );
        assert_eq!(serde_json::from_str::<S>(r#"{"g":5,"s":[7]}"#).unwrap(), s);
        let some = S {
            c: Some(ChannelId::new(6).to_ref()),
            ..s
        };
        assert_eq!(
            serde_json::to_string(&some).unwrap(),
            r#"{"g":5,"c":6,"s":[7]}"#
        );
        assert_eq!(
            serde_json::from_str::<S>(r#"{"g":5,"c":6,"s":[7]}"#).unwrap(),
            some
        );
        assert!(serde_json::from_str::<S>(r#"{"g":5,"c":0,"s":[]}"#).is_err());
        assert!(serde_json::from_str::<S>(r#"{"g":0,"s":[]}"#).is_err());
        assert!(serde_json::from_str::<S>(r#"{"g":5,"s":[0]}"#).is_err());
    }
    #[test]
    fn the_string_adapter_writes_what_serenity_writes() {
        #[derive(Serialize)]
        struct Raw {
            id: GuildId,
        }
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Neutral {
            #[serde(with = "as_snowflake_string")]
            id: CommunityRef,
        }
        let serenity = serde_json::to_string(&Raw {
            id: GuildId::new(123),
        })
        .unwrap();
        let ours = serde_json::to_string(&Neutral {
            id: GuildId::new(123).to_ref(),
        })
        .unwrap();
        assert_eq!(serenity, r#"{"id":"123"}"#);
        assert_eq!(ours, serenity);
        let want = Neutral {
            id: GuildId::new(123).to_ref(),
        };
        assert_eq!(serde_json::from_str::<Neutral>(&serenity).unwrap(), want);
        assert_eq!(
            serde_json::from_str::<Neutral>(r#"{"id":123}"#).unwrap(),
            want
        );
        for bad in [
            r#"{"id":"0"}"#,
            r#"{"id":"18446744073709551615"}"#,
            r#"{"id":"abc"}"#,
            r#"{"id":-1}"#,
            r#"{"id":0}"#,
            r#"{"id":"007"}"#,
            r#"{"id":" 7"}"#,
            r#"{"id":null}"#,
        ] {
            assert!(serde_json::from_str::<Neutral>(bad).is_err(), "{bad}");
        }
        // A type that is neither says what is expected.
        let wrong = serde_json::from_str::<Neutral>(r#"{"id":true}"#).unwrap_err();
        assert!(
            wrong
                .to_string()
                .contains("a snowflake as a string or an integer"),
            "{wrong}"
        );
    }
}
