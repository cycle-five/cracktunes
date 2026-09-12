//! What the music commands need from Discord, and whether we have it.
//!
//! Permissions here fall into two tiers that behave differently:
//!
//! - **Blocking** ([`VOICE_REQUIRED`]) — without these a join cannot work.
//!   Discord accepts the voice state update and silently does nothing with
//!   it, so songbird waits out its timeout and reports `JoinError::TimedOut`,
//!   which names nothing useful. [`ensure_can_join`] refuses first instead.
//! - **Degrading** ([`TEXT_REQUIRED`]) — without these the bot still plays
//!   perfectly; it just cannot announce. Playback needs no text permission at
//!   all, so refusing to play because we cannot post the now-playing message
//!   would be strictly worse for the user than playing silently.

use poise::serenity_prelude as serenity;
use serenity::all::{ChannelId, GenericChannelId, Permissions};

/// Text permissions the now-playing posts need. Missing any of these degrades
/// the bot; it never blocks it.
pub const TEXT_REQUIRED: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::EMBED_LINKS);

/// Voice permissions a join needs. Missing either blocks playback outright.
pub const VOICE_REQUIRED: Permissions = Permissions::CONNECT.union(Permissions::SPEAK);

/// The bot's permissions as they bear on playing music in one guild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicPermissions {
    pub text: TextPerms,
    /// `None` when the author is in no voice channel at all — deliberately a
    /// distinct state from "in a channel we cannot join", because the two
    /// need different messages.
    pub voice: Option<VoicePerms>,
}

/// What the bot may do in the channel a command was invoked in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPerms {
    pub channel: GenericChannelId,
    pub granted: Permissions,
}

/// What the bot may do in the voice channel it would join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePerms {
    pub channel: ChannelId,
    pub granted: Permissions,
}

impl TextPerms {
    /// The subset of [`TEXT_REQUIRED`] we do not have. Renders itself as
    /// comma-separated permission names via serenity's `Display`.
    pub fn missing(&self) -> Permissions {
        TEXT_REQUIRED - self.granted
    }
    pub fn is_whole(&self) -> bool {
        self.missing().is_empty()
    }
    pub fn view(&self) -> bool {
        self.granted.contains(Permissions::VIEW_CHANNEL)
    }
    pub fn send(&self) -> bool {
        self.granted.contains(Permissions::SEND_MESSAGES)
    }
    pub fn embed(&self) -> bool {
        self.granted.contains(Permissions::EMBED_LINKS)
    }
}

impl VoicePerms {
    /// The subset of [`VOICE_REQUIRED`] we do not have.
    pub fn missing(&self) -> Permissions {
        VOICE_REQUIRED - self.granted
    }
    pub fn can_join(&self) -> bool {
        self.missing().is_empty()
    }
    pub fn connect(&self) -> bool {
        self.granted.contains(Permissions::CONNECT)
    }
    pub fn speak(&self) -> bool {
        self.granted.contains(Permissions::SPEAK)
    }
}

/// Build the permission picture from already-resolved bitsets.
///
/// Pure on purpose: no cache, no `await`, no poise `Context`. Every case worth
/// testing is a synthetic [`Permissions`] value, so the model is covered
/// without Discord, a network, or an async runtime. [`resolve`] is the thin
/// layer that reads the cache and calls this.
pub fn compute(
    text_channel: GenericChannelId,
    text_granted: Permissions,
    voice: Option<(ChannelId, Permissions)>,
) -> MusicPermissions {
    MusicPermissions {
        text: TextPerms {
            channel: text_channel,
            granted: text_granted,
        },
        voice: voice.map(|(channel, granted)| VoicePerms { channel, granted }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::serenity::all::{ChannelId, GenericChannelId};

    fn text_ch() -> GenericChannelId {
        GenericChannelId::new(1)
    }
    fn voice_ch() -> ChannelId {
        ChannelId::new(2)
    }

    #[test]
    fn everything_granted_is_whole_and_joinable() {
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), VOICE_REQUIRED)));
        assert!(p.text.is_whole());
        assert!(p.text.missing().is_empty());
        let v = p.voice.expect("voice was supplied");
        assert!(v.can_join());
        assert!(v.missing().is_empty());
    }

    #[test]
    fn a_missing_speak_blocks_the_join_and_names_only_speak() {
        let granted = VOICE_REQUIRED - Permissions::SPEAK;
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), granted)));
        let v = p.voice.expect("voice was supplied");
        assert!(!v.can_join());
        assert_eq!(v.missing(), Permissions::SPEAK);
        assert!(
            v.connect(),
            "CONNECT was granted and must not be reported missing"
        );
        assert!(!v.speak());
    }

    #[test]
    fn both_voice_perms_missing_are_reported_together() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED,
            Some((voice_ch(), Permissions::empty())),
        );
        let v = p.voice.expect("voice was supplied");
        assert_eq!(v.missing(), VOICE_REQUIRED);
        // The copy is built from Display on the whole set, so both names must
        // appear. This is the property the refusal message depends on.
        let rendered = format!("{}", v.missing());
        assert!(rendered.contains("Connect"), "got {rendered}");
        assert!(rendered.contains("Speak"), "got {rendered}");
    }

    #[test]
    fn a_missing_embed_links_degrades_text_but_leaves_voice_joinable() {
        let granted = TEXT_REQUIRED - Permissions::EMBED_LINKS;
        let p = compute(text_ch(), granted, Some((voice_ch(), VOICE_REQUIRED)));
        assert!(!p.text.is_whole());
        assert_eq!(p.text.missing(), Permissions::EMBED_LINKS);
        assert!(p.text.view() && p.text.send() && !p.text.embed());
        assert!(p.voice.expect("voice was supplied").can_join());
    }

    #[test]
    fn no_voice_channel_is_none_not_a_denial() {
        let p = compute(text_ch(), TEXT_REQUIRED, None);
        // 🪤 `None` means "the author is in no voice channel", which is a
        // different thing from "in a channel we cannot join". Rendering it as
        // a denial would tell someone to grant a permission that is already
        // granted.
        assert!(p.voice.is_none());
        assert!(p.text.is_whole());
    }
}
