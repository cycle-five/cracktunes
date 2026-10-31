//! A recording stand-in for [`RoomsDiscord`]: tests assert on what would have
//! been sent.
use super::discord::{CreatedRoom, DiscordError, NewRoom, RoomsDiscord};
use super::mute_hold::VoiceNow;
use super::state::{OverwriteTarget, StoredOverwrite};
use serenity::all::{ChannelId, GuildId, MessageId, UserId};
use serenity::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Create(NewRoom),
    Delete(u64),
    /// channel, overwrite
    Put(u64, StoredOverwrite),
    /// channel, target
    DeleteOverwrite(u64, OverwriteTarget),
    Limit(u64, u8),
    Rename(u64, String),
    /// user, channel (`None` disconnects)
    Move(u64, Option<u64>),
    /// channel, message
    Pin(u64, u64),
    /// user, on
    SetMute(u64, bool),
    /// user
    VoiceNow(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallKind {
    Create,
    Delete,
    Put,
    DeleteOverwrite,
    Limit,
    Rename,
    Move,
    Pin,
    SetMute,
    VoiceNow,
}

impl Call {
    pub fn kind(&self) -> CallKind {
        match self {
            Call::Create(_) => CallKind::Create,
            Call::Delete(_) => CallKind::Delete,
            Call::Put(..) => CallKind::Put,
            Call::DeleteOverwrite(..) => CallKind::DeleteOverwrite,
            Call::Limit(..) => CallKind::Limit,
            Call::Rename(..) => CallKind::Rename,
            Call::Move(..) => CallKind::Move,
            Call::Pin(..) => CallKind::Pin,
            Call::SetMute(..) => CallKind::SetMute,
            Call::VoiceNow(_) => CallKind::VoiceNow,
        }
    }
}

#[derive(Default)]
struct Inner {
    calls: Vec<Call>,
    failures: HashMap<CallKind, VecDeque<DiscordError>>,
    next_channel: u64,
    created_overwrites: Vec<StoredOverwrite>,
    voice: HashMap<UserId, VoiceNow>,
}

/// What the gateway delivers while a call is on its way.
type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct FakeRoomsDiscord {
    inner: Mutex<Inner>,
    before_move: Mutex<Option<Hook>>,
}

impl FakeRoomsDiscord {
    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().unwrap().calls.clone()
    }

    /// The NEXT call of this kind fails (queued per kind); it is still recorded.
    pub fn fail_next(&self, kind: CallKind, e: DiscordError) {
        self.inner
            .lock()
            .unwrap()
            .failures
            .entry(kind)
            .or_default()
            .push_back(e);
    }

    /// `f` runs when the next `move_member` call reaches Discord, before it
    /// answers: a voice event the gateway delivers meanwhile.
    pub fn before_next_move(&self, f: impl FnOnce() + Send + 'static) {
        *self.before_move.lock().unwrap() = Some(Box::new(f));
    }

    /// What every created room comes back with, as if inherited from its category.
    pub fn with_created_overwrites(self, o: Vec<StoredOverwrite>) -> Self {
        self.inner.lock().unwrap().created_overwrites = o;
        self
    }

    /// What `voice_now` answers for `u` from here on (unset: not in voice).
    pub fn set_voice(&self, u: UserId, v: VoiceNow) {
        self.inner.lock().unwrap().voice.insert(u, v);
    }

    fn record(&self, call: Call) -> Result<(), DiscordError> {
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
impl RoomsDiscord for FakeRoomsDiscord {
    async fn create_voice_channel(&self, r: &NewRoom) -> Result<CreatedRoom, DiscordError> {
        self.record(Call::Create(r.clone()))?;
        let mut inner = self.inner.lock().unwrap();
        if inner.next_channel == 0 {
            inner.next_channel = 5000;
        }
        let channel = ChannelId::new(inner.next_channel);
        inner.next_channel += 1;
        Ok(CreatedRoom {
            channel,
            overwrites: inner.created_overwrites.clone(),
        })
    }
    async fn delete_channel(&self, ch: ChannelId) -> Result<(), DiscordError> {
        self.record(Call::Delete(ch.get()))
    }
    async fn put_overwrite(&self, ch: ChannelId, o: &StoredOverwrite) -> Result<(), DiscordError> {
        self.record(Call::Put(ch.get(), *o))
    }
    async fn delete_overwrite(
        &self,
        ch: ChannelId,
        t: OverwriteTarget,
    ) -> Result<(), DiscordError> {
        self.record(Call::DeleteOverwrite(ch.get(), t))
    }
    async fn set_user_limit(&self, ch: ChannelId, limit: u8) -> Result<(), DiscordError> {
        self.record(Call::Limit(ch.get(), limit))
    }
    async fn rename(&self, ch: ChannelId, name: &str) -> Result<(), DiscordError> {
        self.record(Call::Rename(ch.get(), name.to_owned()))
    }
    async fn move_member(
        &self,
        _guild: GuildId,
        user: UserId,
        to: Option<ChannelId>,
    ) -> Result<(), DiscordError> {
        // Taken out first: the hook runs without the lock held.
        let hook = self.before_move.lock().unwrap().take();
        if let Some(f) = hook {
            f();
        }
        self.record(Call::Move(user.get(), to.map(ChannelId::get)))
    }
    async fn pin(&self, ch: ChannelId, msg: MessageId) -> Result<(), DiscordError> {
        self.record(Call::Pin(ch.get(), msg.get()))
    }
    async fn set_mute(&self, _guild: GuildId, user: UserId, on: bool) -> Result<(), DiscordError> {
        self.record(Call::SetMute(user.get(), on))
    }
    async fn voice_now(&self, _guild: GuildId, user: UserId) -> Result<VoiceNow, DiscordError> {
        self.record(Call::VoiceNow(user.get()))?;
        Ok(self
            .inner
            .lock()
            .unwrap()
            .voice
            .get(&user)
            .copied()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::{Permissions, RoleId};

    fn ow() -> StoredOverwrite {
        StoredOverwrite {
            target: OverwriteTarget::Role { id: RoleId::new(9) },
            allow: Permissions::CONNECT,
            deny: Permissions::empty(),
        }
    }

    #[tokio::test]
    async fn fake_records_calls_in_order_and_fails_on_script() {
        let fake = FakeRoomsDiscord::default().with_created_overwrites(vec![ow()]);
        let new = NewRoom {
            guild: GuildId::new(1),
            name: "room".into(),
            parent: Some(ChannelId::new(2)),
            user_limit: 4,
        };
        let a = fake.create_voice_channel(&new).await.unwrap();
        let b = fake.create_voice_channel(&new).await.unwrap();
        assert_eq!(a.channel, ChannelId::new(5000));
        assert_eq!(b.channel, ChannelId::new(5001));
        assert_eq!(a.overwrites, vec![ow()]);

        let ch = ChannelId::new(5000);
        fake.fail_next(CallKind::Rename, DiscordError::MissingPermissions);
        assert_eq!(
            fake.rename(ch, "x").await,
            Err(DiscordError::MissingPermissions)
        );
        // The scripted failure was one-shot, and does not touch other kinds.
        fake.rename(ch, "y").await.unwrap();
        fake.put_overwrite(ch, &ow()).await.unwrap();
        fake.delete_overwrite(ch, ow().target).await.unwrap();
        fake.set_user_limit(ch, 7).await.unwrap();
        fake.move_member(GuildId::new(1), UserId::new(3), None)
            .await
            .unwrap();
        fake.move_member(GuildId::new(1), UserId::new(3), Some(ch))
            .await
            .unwrap();
        fake.pin(ch, MessageId::new(8)).await.unwrap();
        fake.delete_channel(ch).await.unwrap();

        // The two mute calls: voice_now answers what was scripted, else "not in voice".
        let (g, u) = (GuildId::new(1), UserId::new(3));
        assert_eq!(fake.voice_now(g, u).await.unwrap(), VoiceNow::default());
        let here = VoiceNow {
            channel: Some(ch),
            server_muted: true,
        };
        fake.set_voice(u, here);
        assert_eq!(fake.voice_now(g, u).await.unwrap(), here);
        fake.fail_next(CallKind::SetMute, DiscordError::NotConnected);
        assert_eq!(
            fake.set_mute(g, u, false).await,
            Err(DiscordError::NotConnected)
        );
        fake.set_mute(g, u, true).await.unwrap();

        assert_eq!(
            fake.calls(),
            vec![
                Call::Create(new.clone()),
                Call::Create(new),
                Call::Rename(5000, "x".into()),
                Call::Rename(5000, "y".into()),
                Call::Put(5000, ow()),
                Call::DeleteOverwrite(5000, ow().target),
                Call::Limit(5000, 7),
                Call::Move(3, None),
                Call::Move(3, Some(5000)),
                Call::Pin(5000, 8),
                Call::Delete(5000),
                Call::VoiceNow(3),
                Call::VoiceNow(3),
                Call::SetMute(3, false),
                Call::SetMute(3, true),
            ]
        );
    }
}
