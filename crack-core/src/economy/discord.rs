//! The only door the shop uses to change a member: nickname, mute, deafen and
//! move. Tests record exactly what would be sent. Messages go elsewhere.
use super::settings::ActionKind;
use serenity::all::{ChannelId, EditMember, GuildId, Http, UserId};
use serenity::async_trait;
use serenity::http::{HttpError, JsonErrorCode};
use std::sync::Arc;

/// The start of every audit-log reason the shop writes.
pub const SHOP_AUDIT_REASON_PREFIX: &str = "CrackTunes shop";

/// The audit-log reason for a call made on behalf of `kind`.
pub fn audit_reason(kind: ActionKind) -> String {
    format!("{SHOP_AUDIT_REASON_PREFIX}: {}", kind.key())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconDiscordError {
    /// Discord code 50013: the bot lacks a permission, or the member outranks it.
    MissingPermissions,
    /// Discord code 40032: the member is not in voice.
    NotConnected,
    /// Discord code 10007: the member left the server.
    UnknownMember,
    Other(String),
}

impl From<serenity::Error> for EconDiscordError {
    fn from(err: serenity::Error) -> Self {
        if let serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) = &err {
            match response.error.code {
                JsonErrorCode::LackPermissionsForAction => return Self::MissingPermissions,
                JsonErrorCode::TargetUserNotConnectedToVoice => return Self::NotConnected,
                JsonErrorCode::UnknownMember => return Self::UnknownMember,
                _ => {},
            }
        }
        Self::Other(err.to_string())
    }
}

#[async_trait]
pub trait EconomyDiscord: Send + Sync {
    /// `None` clears the nickname.
    async fn set_nick(
        &self,
        g: GuildId,
        u: UserId,
        nick: Option<&str>,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError>;
    async fn set_mute(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError>;
    async fn set_deaf(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError>;
    async fn move_member(
        &self,
        g: GuildId,
        u: UserId,
        to: ChannelId,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError>;
}

/// The real Discord behind [`EconomyDiscord`]. Untested here; the TuneTitan
/// checklist covers it.
pub struct LiveEconomyDiscord {
    pub http: Arc<Http>,
}

impl LiveEconomyDiscord {
    async fn edit(
        &self,
        g: GuildId,
        u: UserId,
        builder: EditMember<'_>,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        g.edit_member(&self.http, u, builder.audit_log_reason(audit_reason(kind)))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl EconomyDiscord for LiveEconomyDiscord {
    async fn set_nick(
        &self,
        g: GuildId,
        u: UserId,
        nick: Option<&str>,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        // An empty nickname clears it.
        self.edit(g, u, EditMember::new().nickname(nick.unwrap_or("")), kind)
            .await
    }

    async fn set_mute(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.edit(g, u, EditMember::new().mute(on), kind).await
    }

    async fn set_deaf(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.edit(g, u, EditMember::new().deafen(on), kind).await
    }

    async fn move_member(
        &self,
        g: GuildId,
        u: UserId,
        to: ChannelId,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.edit(g, u, EditMember::new().voice_channel(to), kind)
            .await
    }
}

/// A recording stand-in for [`EconomyDiscord`]: tests assert on what would
/// have been sent.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Nick(UserId, Option<String>, ActionKind),
        Mute(UserId, bool, ActionKind),
        Deaf(UserId, bool, ActionKind),
        Move(UserId, ChannelId, ActionKind),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum CallKind {
        Nick,
        Mute,
        Deaf,
        Move,
    }

    impl Call {
        pub fn kind(&self) -> CallKind {
            match self {
                Call::Nick(..) => CallKind::Nick,
                Call::Mute(..) => CallKind::Mute,
                Call::Deaf(..) => CallKind::Deaf,
                Call::Move(..) => CallKind::Move,
            }
        }
    }

    #[derive(Default)]
    struct Inner {
        calls: Vec<Call>,
        failures: HashMap<CallKind, VecDeque<EconDiscordError>>,
    }

    #[derive(Default)]
    pub struct FakeEconomyDiscord {
        inner: Mutex<Inner>,
    }

    impl FakeEconomyDiscord {
        pub fn calls(&self) -> Vec<Call> {
            self.inner.lock().unwrap().calls.clone()
        }

        /// The NEXT call of this kind fails (queued per kind); it is still recorded.
        pub fn fail_next(&self, kind: CallKind, e: EconDiscordError) {
            self.inner
                .lock()
                .unwrap()
                .failures
                .entry(kind)
                .or_default()
                .push_back(e);
        }

        fn record(&self, call: Call) -> Result<(), EconDiscordError> {
            let mut inner = self.inner.lock().unwrap();
            let kind = call.kind();
            inner.calls.push(call);
            match inner.failures.get_mut(&kind).and_then(VecDeque::pop_front) {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
    }

    #[async_trait]
    impl EconomyDiscord for FakeEconomyDiscord {
        async fn set_nick(
            &self,
            _g: GuildId,
            u: UserId,
            nick: Option<&str>,
            kind: ActionKind,
        ) -> Result<(), EconDiscordError> {
            self.record(Call::Nick(u, nick.map(str::to_owned), kind))
        }
        async fn set_mute(
            &self,
            _g: GuildId,
            u: UserId,
            on: bool,
            kind: ActionKind,
        ) -> Result<(), EconDiscordError> {
            self.record(Call::Mute(u, on, kind))
        }
        async fn set_deaf(
            &self,
            _g: GuildId,
            u: UserId,
            on: bool,
            kind: ActionKind,
        ) -> Result<(), EconDiscordError> {
            self.record(Call::Deaf(u, on, kind))
        }
        async fn move_member(
            &self,
            _g: GuildId,
            u: UserId,
            to: ChannelId,
            kind: ActionKind,
        ) -> Result<(), EconDiscordError> {
            self.record(Call::Move(u, to, kind))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use serenity::http::ErrorResponse;

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(2);

    async fn http_error(code: u32) -> serenity::Error {
        let body = format!(r#"{{"code": {code}, "message": "nope"}}"#);
        let response = reqwest13::Response::from(
            http::Response::builder()
                .status(403)
                .body(body)
                .expect("a valid response"),
        );
        serenity::Error::Http(HttpError::UnsuccessfulRequest(
            ErrorResponse::from_response(response, http::Method::PATCH).await,
        ))
    }

    #[test]
    fn every_call_sends_the_shop_audit_reason() {
        assert_eq!(SHOP_AUDIT_REASON_PREFIX, "CrackTunes shop");
        for &k in ActionKind::ALL {
            assert_eq!(audit_reason(k), format!("CrackTunes shop: {}", k.key()));
        }
        assert_eq!(audit_reason(ActionKind::Drag), "CrackTunes shop: drag");
    }

    #[tokio::test]
    async fn the_fake_records_calls_and_fails_on_demand() {
        let fake = FakeEconomyDiscord::default();
        let k = ActionKind::Rename;
        fake.set_nick(G, U, Some("x"), k).await.unwrap();
        fake.set_nick(G, U, None, k).await.unwrap();
        fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
        assert_eq!(
            fake.set_mute(G, U, true, ActionKind::Mute).await,
            Err(EconDiscordError::NotConnected)
        );
        // One-shot, and other kinds are untouched.
        fake.set_mute(G, U, false, ActionKind::Mute).await.unwrap();
        fake.set_deaf(G, U, true, ActionKind::Deafen).await.unwrap();
        fake.move_member(G, U, ChannelId::new(7), ActionKind::Drag)
            .await
            .unwrap();
        assert_eq!(
            fake.calls(),
            vec![
                Call::Nick(U, Some("x".into()), k),
                Call::Nick(U, None, k),
                Call::Mute(U, true, ActionKind::Mute),
                Call::Mute(U, false, ActionKind::Mute),
                Call::Deaf(U, true, ActionKind::Deafen),
                Call::Move(U, ChannelId::new(7), ActionKind::Drag),
            ]
        );
    }

    #[tokio::test]
    async fn error_mapping_matches_rooms_codes() {
        assert_eq!(
            EconDiscordError::from(http_error(50013).await),
            EconDiscordError::MissingPermissions
        );
        assert_eq!(
            EconDiscordError::from(http_error(40032).await),
            EconDiscordError::NotConnected
        );
        assert_eq!(
            EconDiscordError::from(http_error(10007).await),
            EconDiscordError::UnknownMember
        );
        match EconDiscordError::from(http_error(10008).await) {
            EconDiscordError::Other(text) => assert!(!text.is_empty()),
            other => panic!("expected Other, got {other:?}"),
        }
    }
}
