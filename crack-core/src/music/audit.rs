//! The queue audit log: who changed a guild's queue, how, and when. Recorded by
//! the queue primitives through their `QueueGuard` (see `music::lease`) and by
//! `music::disconnect`; written to `queue_audit` by `db::queue_audit`.
//! Spec: docs/superpowers/specs/2026-09-30-queue-audit-log-design.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GenericChannelId, GuildId, UserId};
use songbird::tracks::TrackHandle;
use std::borrow::Cow;
use tokio::sync::mpsc;

/// Where an action came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Slash,
    Prefix,
    Web,
    /// A now-playing button (`np:`), pressed in Discord.
    Button,
    Bot,
}

impl Source {
    /// The stored spelling; matches the serde name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Slash => "slash",
            Source::Prefix => "prefix",
            Source::Web => "web",
            Source::Button => "button",
            Source::Bot => "bot",
        }
    }
}

/// Why the bot changed a queue with nobody asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotReason {
    Autopause,
    Autoplay,
    Game,
    IdleTimeout,
    Kicked,
    JoinCleanup,
    /// A restart's queue, rebuilt.
    Resume,
}

impl BotReason {
    fn name(self) -> &'static str {
        match self {
            BotReason::Autopause => "autopause",
            BotReason::Autoplay => "autoplay",
            BotReason::Game => "gp",
            BotReason::IdleTimeout => "idle timeout",
            BotReason::Kicked => "disconnected",
            BotReason::JoinCleanup => "join cleanup",
            BotReason::Resume => "resume",
        }
    }
}

/// Who acted. Built only by the constructors below, so every recorded action
/// names a source that matches how it was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    user: Option<UserId>,
    source: Source,
    command: Cow<'static, str>,
    origin_channel: Option<GenericChannelId>,
}

impl Actor {
    /// The member running this command, in the channel they ran it from.
    #[must_use]
    pub fn from_ctx(ctx: &crate::Context<'_>) -> Self {
        let is_prefix = matches!(ctx, poise::Context::Prefix(_));
        Self::for_command(
            ctx.author().id,
            is_prefix,
            ctx.command().qualified_name.clone(),
            Some(ctx.channel_id()),
        )
    }

    /// A command, spelled out. `from_ctx` is the usual way in.
    #[must_use]
    pub fn for_command(
        user: UserId,
        is_prefix: bool,
        name: impl Into<Cow<'static, str>>,
        origin_channel: Option<GenericChannelId>,
    ) -> Self {
        Self {
            user: Some(user),
            source: if is_prefix {
                Source::Prefix
            } else {
                Source::Slash
            },
            command: name.into(),
            origin_channel,
        }
    }

    /// A signed-in member acting from the web dashboard; `op` names the control.
    #[must_use]
    pub fn web(user: UserId, op: &'static str) -> Self {
        Self {
            user: Some(user),
            source: Source::Web,
            command: Cow::Owned(format!("dashboard {op}")),
            origin_channel: None,
        }
    }

    /// A member pressing a now-playing button; `op` names the control.
    #[must_use]
    pub fn button(user: UserId, op: &'static str) -> Self {
        Self {
            user: Some(user),
            source: Source::Button,
            command: Cow::Owned(format!("button {op}")),
            origin_channel: None,
        }
    }

    /// The bot acting on its own.
    #[must_use]
    pub fn bot(reason: BotReason) -> Self {
        Self {
            user: None,
            source: Source::Bot,
            command: Cow::Borrowed(reason.name()),
            origin_channel: None,
        }
    }

    #[must_use]
    pub fn user(&self) -> Option<UserId> {
        self.user
    }
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }
    #[must_use]
    pub fn origin_channel(&self) -> Option<GenericChannelId> {
        self.origin_channel
    }
}

/// A track as the log remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Where an add put its tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddAt {
    Front,
    Back,
    Index(usize),
}

/// What happened to the queue. Stored whole as `detail`, with its tag also in
/// the `action` column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Add {
        tracks: Vec<TrackRef>,
        at: AddAt,
    },
    Remove {
        track: TrackRef,
        index: usize,
    },
    Move {
        track: TrackRef,
        from: usize,
        to: usize,
    },
    Skip {
        track: Option<TrackRef>,
    },
    Clear {
        removed: usize,
    },
    Shuffle {
        count: usize,
    },
    Stop {
        removed: usize,
    },
    Pause,
    Resume,
    Repeat {
        on: bool,
    },
    Leave {
        discarded: usize,
    },
}

impl Action {
    /// The serde tag, for the `action` column.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Action::Add { .. } => "add",
            Action::Remove { .. } => "remove",
            Action::Move { .. } => "move",
            Action::Skip { .. } => "skip",
            Action::Clear { .. } => "clear",
            Action::Shuffle { .. } => "shuffle",
            Action::Stop { .. } => "stop",
            Action::Pause => "pause",
            Action::Resume => "resume",
            Action::Repeat { .. } => "repeat",
            Action::Leave { .. } => "leave",
        }
    }
}

/// One row of `queue_audit`.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub at: DateTime<Utc>,
    pub guild_id: GuildId,
    pub voice_channel: Option<ChannelId>,
    pub actor: Actor,
    pub action: Action,
}

/// Title and URL of a queued track, read without waiting.
///
/// The primitives run inside `modify_queue`'s synchronous closure, so this
/// uses `try_read`. The metadata lock is written once, as the track is built,
/// so contention is not expected; if it happens, the track is recorded without
/// a title rather than the queue waiting.
#[must_use]
pub fn track_ref(track: &TrackHandle) -> TrackRef {
    let data = crate::utils::track_data(track);
    let (title, url) = match data.aux_metadata.try_read() {
        Ok(meta) => (
            meta.as_ref().and_then(|m| m.title.clone()),
            meta.as_ref().and_then(|m| m.source_url.clone()),
        ),
        Err(_) => (None, None),
    };
    TrackRef { title, url }
}

/// Hand an event to the writer. Never waits and never fails: a full channel
/// drops the event with a warning, and with no writer (no database) the event
/// goes to the log instead.
pub fn emit(tx: Option<&mpsc::Sender<AuditEvent>>, event: AuditEvent) {
    match tx {
        Some(tx) => {
            if let Err(e) = tx.try_send(event) {
                tracing::warn!("queue audit: dropped an event ({e})");
            }
        },
        None => tracing::info!(
            guild = %event.guild_id,
            user = ?event.actor.user(),
            source = event.actor.source().as_str(),
            command = event.actor.command(),
            action = event.action.name(),
            "queue audit (no database)"
        ),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// `as_str` is what the database stores; serde is what the wire says. They
    /// are written twice, so this is what keeps them one spelling.
    #[test]
    fn source_as_str_is_the_serde_name() {
        for source in [
            Source::Slash,
            Source::Prefix,
            Source::Web,
            Source::Button,
            Source::Bot,
        ] {
            let json = serde_json::to_string(&source).unwrap();
            assert_eq!(json.trim_matches('"'), source.as_str());
        }
    }

    #[test]
    fn a_button_press_is_its_own_source() {
        let a = Actor::button(UserId::new(42), "skip");
        assert_eq!(a.source(), Source::Button);
        assert_eq!(a.source().as_str(), "button");
        assert_eq!(a.command(), "button skip");
        assert_eq!(a.user(), Some(UserId::new(42)));
        assert_eq!(a.origin_channel(), None);
        // The stored spelling is the serde name.
        assert_eq!(
            serde_json::to_string(&Source::Button).unwrap(),
            "\"button\""
        );
    }

    #[test]
    fn the_constructors_set_source_user_and_command() {
        let slash = Actor::for_command(
            UserId::new(7),
            false,
            "skip",
            Some(GenericChannelId::new(3)),
        );
        assert_eq!(
            (slash.source(), slash.user(), slash.command()),
            (Source::Slash, Some(UserId::new(7)), "skip")
        );
        assert_eq!(slash.origin_channel(), Some(GenericChannelId::new(3)));
        let prefix = Actor::for_command(UserId::new(7), true, "skip", None);
        assert_eq!(prefix.source(), Source::Prefix);
        let web = Actor::web(UserId::new(8), "move");
        assert_eq!(
            (web.source(), web.user(), web.command()),
            (Source::Web, Some(UserId::new(8)), "dashboard move")
        );
        assert_eq!(web.origin_channel(), None);
        let bot = Actor::bot(BotReason::IdleTimeout);
        assert_eq!(
            (bot.source(), bot.user(), bot.command()),
            (Source::Bot, None, "idle timeout")
        );
    }

    #[test]
    fn every_action_names_its_own_serde_tag() {
        #[derive(serde::Deserialize)]
        struct Tag {
            action: String,
        }
        let t = TrackRef {
            title: Some("a".into()),
            url: None,
        };
        let all = vec![
            Action::Add {
                tracks: vec![t.clone()],
                at: AddAt::Back,
            },
            Action::Remove {
                track: t.clone(),
                index: 1,
            },
            Action::Move {
                track: t.clone(),
                from: 1,
                to: 2,
            },
            Action::Skip {
                track: Some(t.clone()),
            },
            Action::Clear { removed: 2 },
            Action::Shuffle { count: 3 },
            Action::Stop { removed: 1 },
            Action::Pause,
            Action::Resume,
            Action::Repeat { on: true },
            Action::Leave { discarded: 4 },
        ];
        for a in all {
            let tag: Tag = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
            assert_eq!(tag.action, a.name());
        }
    }

    #[test]
    fn repeat_round_trips_with_its_tag() {
        let a = Action::Repeat { on: true };
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s, r#"{"action":"repeat","on":true}"#);
        assert_eq!(serde_json::from_str::<Action>(&s).unwrap(), a);
        assert_eq!(a.name(), "repeat");
    }

    #[test]
    fn a_move_serializes_with_its_fields() {
        let a = Action::Move {
            track: TrackRef {
                title: Some("t".into()),
                url: Some("https://x".into()),
            },
            from: 3,
            to: 1,
        };
        let back: Action = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }

    fn event() -> AuditEvent {
        AuditEvent {
            at: chrono::Utc::now(),
            guild_id: GuildId::new(1),
            voice_channel: None,
            actor: Actor::bot(BotReason::Autopause),
            action: Action::Pause,
        }
    }

    #[test]
    fn emit_sends_the_event() {
        let (tx, mut rx) = mpsc::channel(4);
        emit(Some(&tx), event());
        assert_eq!(rx.try_recv().unwrap().action, Action::Pause);
    }

    #[tokio::test]
    async fn a_full_channel_drops_without_waiting() {
        let (tx, mut rx) = mpsc::channel(1);
        emit(Some(&tx), event());
        // The channel is full. This must return at once, not wait for room.
        tokio::time::timeout(std::time::Duration::from_millis(50), async {
            emit(Some(&tx), event())
        })
        .await
        .expect("emit must not wait on a full channel");
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "the second event was dropped");
    }

    #[test]
    fn no_sender_is_not_an_error() {
        emit(None, event());
    }
}
