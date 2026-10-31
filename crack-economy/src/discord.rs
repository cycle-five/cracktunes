//! The only door the shop uses to change a member: nickname, mute, deafen and
//! move; and the one fresh read of a member's nickname reconcile makes. Tests
//! record exactly what would be sent. Messages go elsewhere.
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
    /// The member's nickname as Discord has it now, never the cache's copy
    /// (`None`: no nickname).
    async fn member_nick(&self, g: GuildId, u: UserId) -> Result<Option<String>, EconDiscordError>;
}

/// One change the shop made to a member, as the mod log reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShopChange {
    /// `None`: the nickname was cleared.
    Nick(Option<String>),
    Mute(bool),
    Deaf(bool),
    Move(ChannelId),
}

/// Told of every change the shop made to a member, after Discord accepted
/// it. The change has no buyer: the actor is CrackTunes, as in Discord's own
/// audit log. Synchronous: an implementation that does I/O spawns it.
pub trait ShopChangeLog: Send + Sync {
    fn changed(&self, g: GuildId, u: UserId, change: ShopChange, kind: ActionKind);
}

/// An [`EconomyDiscord`] that reports each *successful* change to a
/// [`ShopChangeLog`]. A failed call changed nothing, so it reports nothing;
/// the nickname read is not a change.
pub struct Logged<D> {
    inner: D,
    log: Arc<dyn ShopChangeLog>,
}

impl<D> Logged<D> {
    pub fn new(inner: D, log: Arc<dyn ShopChangeLog>) -> Self {
        Self { inner, log }
    }
}

#[async_trait]
impl<D: EconomyDiscord> EconomyDiscord for Logged<D> {
    async fn set_nick(
        &self,
        g: GuildId,
        u: UserId,
        nick: Option<&str>,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.inner.set_nick(g, u, nick, kind).await?;
        self.log
            .changed(g, u, ShopChange::Nick(nick.map(str::to_owned)), kind);
        Ok(())
    }

    async fn set_mute(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.inner.set_mute(g, u, on, kind).await?;
        self.log.changed(g, u, ShopChange::Mute(on), kind);
        Ok(())
    }

    async fn set_deaf(
        &self,
        g: GuildId,
        u: UserId,
        on: bool,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.inner.set_deaf(g, u, on, kind).await?;
        self.log.changed(g, u, ShopChange::Deaf(on), kind);
        Ok(())
    }

    async fn move_member(
        &self,
        g: GuildId,
        u: UserId,
        to: ChannelId,
        kind: ActionKind,
    ) -> Result<(), EconDiscordError> {
        self.inner.move_member(g, u, to, kind).await?;
        self.log.changed(g, u, ShopChange::Move(to), kind);
        Ok(())
    }

    async fn member_nick(&self, g: GuildId, u: UserId) -> Result<Option<String>, EconDiscordError> {
        self.inner.member_nick(g, u).await
    }
}

/// The real Discord behind [`EconomyDiscord`], which only ever exists wrapped
/// in [`Logged`] (see [`LiveEconomyDiscord::logged`]): every change the shop
/// makes reaches the mod log. Untested here; the TuneTitan checklist covers it.
pub struct LiveEconomyDiscord {
    http: Arc<Http>,
}

/// The only live door there is.
pub type LiveShopDiscord = Logged<LiveEconomyDiscord>;

impl LiveEconomyDiscord {
    /// The live door, already reporting to `log`. There is no way to build one
    /// that doesn't.
    pub fn logged(http: Arc<Http>, log: Arc<dyn ShopChangeLog>) -> LiveShopDiscord {
        Logged::new(Self { http }, log)
    }

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

    async fn member_nick(&self, g: GuildId, u: UserId) -> Result<Option<String>, EconDiscordError> {
        // `Http` holds no cache, so this asks Discord.
        let member = g.member(&*self.http, u).await?;
        Ok(member.nick.as_ref().map(ToString::to_string))
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
        /// What `member_nick` answers per member; unset fails.
        nicks: HashMap<UserId, Option<String>>,
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

        /// What Discord says `u`'s nickname is, for `member_nick`. A member
        /// never set can't be fetched.
        pub fn set_member_nick(&self, u: UserId, nick: Option<&str>) {
            self.inner
                .lock()
                .unwrap()
                .nicks
                .insert(u, nick.map(str::to_owned));
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
        async fn member_nick(
            &self,
            _g: GuildId,
            u: UserId,
        ) -> Result<Option<String>, EconDiscordError> {
            self.inner
                .lock()
                .unwrap()
                .nicks
                .get(&u)
                .cloned()
                .ok_or_else(|| EconDiscordError::Other("no such member".into()))
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

    /// What a [`ShopChangeLog`] was told, in order.
    #[derive(Default)]
    struct Recorder(std::sync::Mutex<Vec<(UserId, ShopChange, ActionKind)>>);

    impl ShopChangeLog for Recorder {
        fn changed(&self, _g: GuildId, u: UserId, change: ShopChange, kind: ActionKind) {
            self.0.lock().unwrap().push((u, change, kind));
        }
    }

    #[tokio::test]
    async fn logged_reports_each_success_once_and_no_failure() {
        let rec = Arc::new(Recorder::default());
        let door = Logged::new(FakeEconomyDiscord::default(), rec.clone());
        let to = ChannelId::new(7);
        door.set_nick(G, U, Some("x"), ActionKind::Rename)
            .await
            .unwrap();
        door.set_mute(G, U, true, ActionKind::Mute).await.unwrap();
        door.set_deaf(G, U, false, ActionKind::Deafen)
            .await
            .unwrap();
        door.move_member(G, U, to, ActionKind::Jail).await.unwrap();
        let ok = vec![
            (U, ShopChange::Nick(Some("x".into())), ActionKind::Rename),
            (U, ShopChange::Mute(true), ActionKind::Mute),
            (U, ShopChange::Deaf(false), ActionKind::Deafen),
            (U, ShopChange::Move(to), ActionKind::Jail),
        ];
        assert_eq!(*rec.0.lock().unwrap(), ok);

        // Each kind now fails once: nothing more is reported.
        let fake = &door.inner;
        for kind in [
            CallKind::Nick,
            CallKind::Mute,
            CallKind::Deaf,
            CallKind::Move,
        ] {
            fake.fail_next(kind, EconDiscordError::NotConnected);
        }
        assert!(door.set_nick(G, U, None, ActionKind::Rename).await.is_err());
        assert!(door.set_mute(G, U, false, ActionKind::Mute).await.is_err());
        assert!(door.set_deaf(G, U, true, ActionKind::Deafen).await.is_err());
        assert!(door.move_member(G, U, to, ActionKind::Jail).await.is_err());
        assert_eq!(*rec.0.lock().unwrap(), ok);
    }

    #[tokio::test]
    async fn member_nick_is_never_reported() {
        let rec = Arc::new(Recorder::default());
        let fake = FakeEconomyDiscord::default();
        fake.set_member_nick(U, Some("n"));
        let door = Logged::new(fake, rec.clone());
        assert_eq!(door.member_nick(G, U).await, Ok(Some("n".into())));
        assert!(rec.0.lock().unwrap().is_empty());
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
