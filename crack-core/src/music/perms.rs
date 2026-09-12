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
use serenity::all::{Cache, ChannelId, GenericChannelId, GuildId, Permissions, UserId};

use crate::errors::{CrackedError, PermScope};

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

/// Read the bot's permissions out of the cache.
///
/// Returns `None` when the guild, the bot's own member, or the text channel is
/// not cached — callers treat that as "assume fine" rather than refusing.
///
/// No HTTP on this path. `GatewayIntents::GUILD_MEMBERS` is enabled
/// (`config.rs:321`), so the bot's own `Member` is cached and permissions
/// resolve locally. That matters because [`ensure_can_join`] runs on every
/// join, and a per-join round trip would be a latency and rate-limit
/// regression.
pub fn resolve(
    cache: &Cache,
    guild_id: GuildId,
    text_channel: GenericChannelId,
    author: UserId,
) -> Option<MusicPermissions> {
    let bot_id = cache.current_user().id;
    let guild = cache.guild(guild_id)?;
    let bot = guild.members.get(&bot_id)?;

    let text_chan = guild.channels.get(&text_channel.expect_channel())?;
    let text_granted = guild.user_permissions_in(text_chan, bot);

    // The author's voice channel, if they are in one. Absent is not a denial.
    let voice = guild
        .voice_states
        .get(&author)
        .and_then(|vs| vs.channel_id)
        .and_then(|cid| {
            let chan = guild.channels.get(&cid)?;
            Some((cid, guild.user_permissions_in(chan, bot)))
        });

    Some(compute(text_channel, text_granted, voice))
}

/// The blocking gate: refuse a join Discord would silently drop.
///
/// Call this immediately before every `songbird.join`. It takes cache handles
/// rather than a poise `Context` on purpose — one of its call sites is a
/// restart-resume with no invoking user and therefore no `Context`.
///
/// Fails **open** on a cache miss: an unknown permission state must not refuse
/// a join that would have worked.
pub fn ensure_can_join(
    cache: &Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), CrackedError> {
    let Some(guild) = cache.guild(guild_id) else {
        return Ok(());
    };
    let bot_id = cache.current_user().id;
    let Some(bot) = guild.members.get(&bot_id) else {
        return Ok(());
    };
    let Some(chan) = guild.channels.get(&channel_id) else {
        return Ok(());
    };

    let missing = VOICE_REQUIRED - guild.user_permissions_in(chan, bot);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(CrackedError::MissingBotPermissions {
            scope: PermScope::Voice,
            channel: channel_id.widen(),
            missing,
        })
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

    // `resolve` and `ensure_can_join` read a live serenity Cache, which cannot
    // be constructed meaningfully offline, so their *logic* is tested through
    // `compute` above and their *placement* by the source-scan guard in
    // Task 4. What is tested here is the one decision that is neither:
    // what they do when the cache cannot answer.

    #[test]
    fn the_gate_fails_open_when_the_cache_is_empty() {
        let cache = serenity::all::Cache::new();
        // No guild cached, so nothing can be known about permissions.
        let res = ensure_can_join(&cache, serenity::all::GuildId::new(1), voice_ch());
        // 🪤 Fail OPEN, not closed. A cache miss means "we don't know", and
        // refusing a join we could have made would be a worse bug than the
        // timeout this gate exists to replace -- it would break working
        // guilds during the cache warm-up after every restart.
        assert!(
            res.is_ok(),
            "a cache miss must not refuse a join: {:?}",
            res.err().map(|e| e.to_string())
        );
    }

    #[test]
    fn resolving_an_uncached_guild_yields_none() {
        let cache = serenity::all::Cache::new();
        let got = resolve(
            &cache,
            serenity::all::GuildId::new(1),
            text_ch(),
            serenity::all::UserId::new(7),
        );
        assert!(got.is_none(), "an uncached guild has no answer to give");
    }
}

/// 🪤 This module exists because the obvious test — "assert the three join
/// sites are guarded" — is a survey of what was found on 2026-09-12, not a
/// property. It would pass while a fourth site was live.
///
/// That is not hypothetical. In v0.9.5 a guard test for play-history writes
/// asserted a hard-coded count of the three call sites then known, passed,
/// and the bug it was written to prevent was live the entire time, because
/// playlists routed through a fourth path. Pinning the whole surface instead
/// immediately turned up two more entry points.
///
/// So this DERIVES the set of join sites by scanning source, and asserts each
/// one it finds is gated. Adding a fourth join site fails this test until it
/// is gated too.
#[cfg(test)]
mod join_site_guard_tests {
    /// Every file that may contain a `songbird` join. Adding a file here is
    /// cheap; forgetting one is the failure mode, so the assertion below also
    /// requires the total to be non-zero — a scan that silently finds nothing
    /// and passes is worse than no test at all (ct#448, #449, #471).
    const SOURCES: &[(&str, &str)] = &[
        (
            "commands/music_utils.rs",
            include_str!("../commands/music_utils.rs"),
        ),
        (
            "commands/music/gp_persist.rs",
            include_str!("../commands/music/gp_persist.rs"),
        ),
        ("poise_ext.rs", include_str!("../poise_ext.rs")),
    ];

    /// The call that must precede every join.
    const GATE: &str = "ensure_can_join(";

    /// How far back from a join to look for its gate. Generous enough to span
    /// a `let Some(..) = ... else` or a log line in between, tight enough that
    /// a gate on an unrelated earlier join cannot satisfy a later one.
    const WINDOW: usize = 1200;

    /// Finds `.join(` calls that are songbird joins. String `.join(" ")` and
    /// friends are excluded by requiring the first argument to not be a
    /// literal.
    fn songbird_join_offsets(src: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(rel) = src[from..].find(".join(") {
            let at = from + rel;
            let arg = src[at + ".join(".len()..].trim_start();
            // `.join(" ")`, `.join("\n")`, `.join(", ")` are slice joins.
            if !arg.starts_with('"') {
                out.push(at);
            }
            from = at + ".join(".len();
        }
        out
    }

    #[test]
    fn every_songbird_join_is_preceded_by_the_gate() {
        let mut checked = 0usize;
        for (name, src) in SOURCES {
            for at in songbird_join_offsets(src) {
                checked += 1;
                let start = at.saturating_sub(WINDOW);
                let before = &src[start..at];
                assert!(
                    before.contains(GATE),
                    "{name}: a songbird join at byte {at} is not preceded by \
                     `{GATE}` within {WINDOW} bytes.\n\n\
                     Every join must be gated, or a guild missing CONNECT or \
                     SPEAK gets songbird's ~10s JoinError::TimedOut, which \
                     names nothing. Add the gate rather than widening this \
                     test.\n\n\
                     Context:\n{}",
                    &src[start.max(at.saturating_sub(300))..at]
                );
            }
        }
        assert!(
            checked >= 3,
            "expected at least the 3 known songbird join sites, scanned \
             {checked} -- the scan found less than it should, which means it \
             has stopped checking rather than that the joins are gone. Fix \
             the scan."
        );
    }
}
