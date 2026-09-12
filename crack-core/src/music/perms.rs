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
use serenity::all::{Cache, ChannelId, GenericChannelId, GuildId, Permissions, RoleId, UserId};

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
    pub voice: VoiceCheck,
}

/// What we were able to learn about the voice channel the author is sitting in.
///
/// Three states, not two. 🪤 Collapsing "in a channel we cannot see" into "in
/// no channel" is the bug this enum exists to prevent. Discord omits channels
/// the bot lacks `VIEW_CHANNEL` on from `GUILD_CREATE`, so such a channel is
/// simply absent from `guild.channels`; with only two states that came out as
/// "you're not in a voice channel" to someone who was plainly in one, and
/// [`ensure_can_join`] waved the join through into songbird's ~10s
/// `JoinError::TimedOut` — the exact symptom this module exists to delete.
///
/// Named `VoiceCheck` rather than `VoiceState` because serenity already has a
/// `VoiceState`, and [`resolve`] reads a map of those one line away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceCheck {
    /// The author is in no voice channel. Not a denial, and not a problem.
    NotInVoice,
    /// The author is in a channel that is absent from an **already-cached**
    /// guild. That absence is information, not ignorance: Discord would have
    /// sent the channel if the bot could see it.
    Unreadable(ChannelId),
    /// Read successfully.
    Resolved(VoicePerms),
}

impl VoiceCheck {
    /// The resolved permissions, if we got that far. `None` for both of the
    /// other states — callers that need to tell them apart must match.
    pub fn perms(&self) -> Option<&VoicePerms> {
        match self {
            Self::Resolved(v) => Some(v),
            Self::NotInVoice | Self::Unreadable(_) => None,
        }
    }

    /// Build the resolved case from a raw bitset.
    pub fn resolved(channel: ChannelId, granted: Permissions) -> Self {
        Self::Resolved(VoicePerms { channel, granted })
    }
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
    voice: VoiceCheck,
) -> MusicPermissions {
    MusicPermissions {
        text: TextPerms {
            channel: text_channel,
            granted: text_granted,
        },
        voice,
    }
}

/// Whether every role the permission math depends on is actually in the cache.
///
/// 🪤 `Guild::user_permissions_in` has no way to say "unknown role". When a
/// role id on the member is absent from `guild.roles`, or the `@everyone` role
/// (whose id is the guild id) is, serenity substitutes `Permissions::empty()`
/// for it and only logs — see `user_permissions_in_` in serenity's
/// `model/guild/mod.rs`, the `@everyone role missing` error and the
/// `has non-existent role` warning. The bitset that comes back is then
/// **under**-reported, which would make the gate refuse a join that would have
/// worked and send an admin to grant a permission they had already granted.
/// That is the fail-open constraint violated in the worst direction, so both
/// cache readers below check this first and give up rather than guess.
///
/// Takes a lookup closure rather than a `Guild` so the rule is pure and
/// testable without a populated [`Cache`].
pub fn roles_resolvable(
    guild_id: GuildId,
    member_roles: &[RoleId],
    is_known: impl Fn(RoleId) -> bool,
) -> bool {
    is_known(RoleId::new(guild_id.get())) && member_roles.iter().all(|r| is_known(*r))
}

/// Classify the author's voice situation from already-looked-up facts.
///
/// Pure, so the distinction between "in no voice channel" and "in a channel
/// we cannot see" is pinned without a populated [`Cache`]. 🪤 That
/// distinction is the whole point of [`VoiceCheck`], and it lives **here**,
/// not in [`compute`] — which only carries a `VoiceCheck` someone else
/// decided. Leaving the decision inline in [`resolve`]'s cache reads left the
/// one line that matters untested: flipping it back to `NotInVoice`
/// reproduced the original bug with the whole suite still green.
///
/// `granted_in` is called only for a channel `channel_known` accepted, which
/// is why callers may use a lookup that has no answer otherwise. The
/// `granted_in_is_not_consulted_for_a_channel_we_cannot_see` test pins that
/// order: reversing it would turn an unseeable channel into `Resolved` with
/// an empty bitset, refusing for CONNECT and SPEAK instead of VIEW_CHANNEL.
pub fn classify_voice(
    author_channel: Option<ChannelId>,
    channel_known: impl Fn(ChannelId) -> bool,
    granted_in: impl Fn(ChannelId) -> Permissions,
) -> VoiceCheck {
    let Some(cid) = author_channel else {
        return VoiceCheck::NotInVoice;
    };
    if !channel_known(cid) {
        return VoiceCheck::Unreadable(cid);
    }
    VoiceCheck::resolved(cid, granted_in(cid))
}

/// Classify a join target from already-looked-up facts.
///
/// The same extraction as [`classify_voice`], for the same reason: the
/// channel-miss arm is the line that decides whether the gate refuses or
/// waves a join through, and inline in [`join_lookup`]'s cache reads nothing
/// could pin it.
///
/// `guild_readable` is false when nothing is known — the guild is not cached,
/// the bot's own member is not cached, or [`roles_resolvable`] said no.
/// `granted_in` is called only for a channel `channel_known` accepted.
pub fn classify_join(
    channel: ChannelId,
    guild_readable: bool,
    channel_known: impl Fn(ChannelId) -> bool,
    granted_in: impl Fn(ChannelId) -> Permissions,
) -> JoinLookup {
    if !guild_readable {
        return JoinLookup::Unknown;
    }
    if !channel_known(channel) {
        return JoinLookup::Withheld;
    }
    JoinLookup::Granted(granted_in(channel))
}

/// Read the bot's permissions out of the cache.
///
/// Returns `None` when the guild, the bot's own member, or the text channel is
/// not cached, or when the bot's roles cannot all be resolved — callers treat
/// that as "assume fine" rather than refusing.
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

    // An under-reported bitset is worse than no answer at all: see
    // [`roles_resolvable`].
    if !roles_resolvable(guild_id, &bot.roles, |r| guild.roles.get(&r).is_some()) {
        return None;
    }

    let text_chan = guild.channels.get(&text_channel.expect_channel())?;
    let text_granted = guild.user_permissions_in(text_chan, bot);

    // The author's voice channel, if they are in one. Absent from the voice
    // states is not a denial; absent from an already-cached guild's channel
    // list is a different thing entirely, and gets its own state.
    //
    // 🪤 The classification is [`classify_voice`], not inline here. Discord
    // omits the channels the bot lacks VIEW_CHANNEL on from GUILD_CREATE, so
    // a channel missing from an already-cached guild is the answer rather
    // than the absence of one -- and that call is exactly the line that has
    // to stay pinned by a test.
    let voice = classify_voice(
        guild.voice_states.get(&author).and_then(|vs| vs.channel_id),
        |cid| guild.channels.get(&cid).is_some(),
        |cid| {
            guild
                .channels
                .get(&cid)
                .map(|chan| guild.user_permissions_in(chan, bot))
                .unwrap_or_else(Permissions::empty)
        },
    );

    Some(compute(text_channel, text_granted, voice))
}

/// The refusal a set of granted voice permissions earns, if any.
///
/// Pure, so the gate's actual decision — what counts as a refusal, and what
/// it says — is testable without a populated [`Cache`]. [`ensure_can_join`]
/// is the thin layer that reads the cache and calls this, the same split
/// [`compute`] and [`resolve`] already use.
pub fn voice_refusal(channel: ChannelId, granted: Permissions) -> Option<CrackedError> {
    let missing = VOICE_REQUIRED - granted;
    if missing.is_empty() {
        None
    } else {
        Some(CrackedError::MissingBotPermissions {
            scope: PermScope::Voice,
            channel: channel.widen(),
            missing,
        })
    }
}

/// The refusal a voice channel we cannot even see earns.
///
/// Legitimate only against an **already-cached** guild, which is what makes it
/// compatible with failing open: we are not guessing at a permission state we
/// could not read, we are reading Discord's decision not to send us the
/// channel.
///
/// 🪤 Names `VIEW_CHANNEL` and nothing else. `CONNECT` and `SPEAK` may well be
/// granted here — there is no way to compute them for a channel Discord never
/// sent — and naming a permission that is already granted is precisely the
/// failure this module exists to avoid.
pub fn unreadable_refusal(channel: ChannelId) -> CrackedError {
    CrackedError::MissingBotPermissions {
        scope: PermScope::Voice,
        channel: channel.widen(),
        missing: Permissions::VIEW_CHANNEL,
    }
}

/// The blocking gate: refuse a join Discord would silently drop.
///
/// Call this immediately before every `songbird.join`.
///
/// A thin cache layer over [`join_refusal`], which holds the actual rule.
pub fn ensure_can_join(
    cache: &Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), CrackedError> {
    match join_refusal(channel_id, join_lookup(cache, guild_id, channel_id)) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// What the cache was able to say about the channel a join is aimed at.
///
/// The three cases are deliberately not `Option<Permissions>`: "we know
/// nothing" and "we know Discord withheld this channel" demand opposite
/// answers, and flattening them into one `None` is the defect [`VoiceCheck`]
/// exists to prevent, one layer down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinLookup {
    /// Genuine ignorance — the guild is not cached, the bot's own member is
    /// not cached, or one of the bot's roles cannot be resolved.
    Unknown,
    /// The guild **is** cached and the channel is absent from it, which
    /// Discord does exactly when the bot lacks `VIEW_CHANNEL` there.
    Withheld,
    /// Resolved.
    Granted(Permissions),
}

/// The decision the gate makes once the cache has had its say.
///
/// Pure, so all three outcomes — fail open, refuse for `VIEW_CHANNEL`, refuse
/// for `CONNECT`/`SPEAK` — are testable without a populated [`Cache`], the
/// same split [`compute`]/[`resolve`] already use. It is also the split that
/// matters most here: the two-state version of this decision answered
/// `Ok(())` for [`JoinLookup::Withheld`] and nothing could catch it.
pub fn join_refusal(channel: ChannelId, lookup: JoinLookup) -> Option<CrackedError> {
    match lookup {
        // 🪤 Fail OPEN. An unknown permission state must not refuse a join
        // that would have worked; that would break working guilds during the
        // cache warm-up after every restart.
        JoinLookup::Unknown => None,
        // 🪤 NOT a cache miss, and this is the one asymmetry worth its own
        // state. The guild is cached, so its channel list is as complete as
        // the bot is allowed to see it; a channel absent from that list is
        // one Discord withheld. Refusing here is reading an answer, not
        // guessing at one, so it does not violate fail-open. Returning
        // `Ok(())` instead is what let the join run on into songbird's ~10s
        // `JoinError::TimedOut`, naming nothing.
        JoinLookup::Withheld => Some(unreadable_refusal(channel)),
        JoinLookup::Granted(granted) => voice_refusal(channel, granted),
    }
}

/// Ask the cache what it knows about a join target. No HTTP, no `await`.
///
/// Takes cache handles rather than a poise `Context` on purpose — one of
/// [`ensure_can_join`]'s call sites is a restart-resume with no invoking user
/// and therefore no `Context`.
fn join_lookup(cache: &Cache, guild_id: GuildId, channel_id: ChannelId) -> JoinLookup {
    let Some(guild) = cache.guild(guild_id) else {
        return JoinLookup::Unknown;
    };
    let bot_id = cache.current_user().id;
    let Some(bot) = guild.members.get(&bot_id) else {
        return JoinLookup::Unknown;
    };
    classify_join(
        channel_id,
        // An under-reported bitset would refuse a join that works: see
        // [`roles_resolvable`].
        roles_resolvable(guild_id, &bot.roles, |r| guild.roles.get(&r).is_some()),
        |cid| guild.channels.get(&cid).is_some(),
        |cid| {
            guild
                .channels
                .get(&cid)
                .map(|chan| guild.user_permissions_in(chan, bot))
                .unwrap_or_else(Permissions::empty)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::serenity::all::{ChannelId, GenericChannelId, RoleId};

    fn text_ch() -> GenericChannelId {
        GenericChannelId::new(1)
    }
    fn voice_ch() -> ChannelId {
        ChannelId::new(2)
    }

    #[test]
    fn everything_granted_is_whole_and_joinable() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED,
            VoiceCheck::resolved(voice_ch(), VOICE_REQUIRED),
        );
        assert!(p.text.is_whole());
        assert!(p.text.missing().is_empty());
        let v = p.voice.perms().expect("voice was supplied");
        assert!(v.can_join());
        assert!(v.missing().is_empty());
    }

    #[test]
    fn a_missing_speak_blocks_the_join_and_names_only_speak() {
        let granted = VOICE_REQUIRED - Permissions::SPEAK;
        let p = compute(
            text_ch(),
            TEXT_REQUIRED,
            VoiceCheck::resolved(voice_ch(), granted),
        );
        let v = p.voice.perms().expect("voice was supplied");
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
            VoiceCheck::resolved(voice_ch(), Permissions::empty()),
        );
        let v = p.voice.perms().expect("voice was supplied");
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
        let p = compute(
            text_ch(),
            granted,
            VoiceCheck::resolved(voice_ch(), VOICE_REQUIRED),
        );
        assert!(!p.text.is_whole());
        assert_eq!(p.text.missing(), Permissions::EMBED_LINKS);
        assert!(p.text.view() && p.text.send() && !p.text.embed());
        assert!(p.voice.perms().expect("voice was supplied").can_join());
    }

    #[test]
    fn no_voice_channel_is_its_own_state_not_a_denial() {
        let p = compute(text_ch(), TEXT_REQUIRED, VoiceCheck::NotInVoice);
        // 🪤 `NotInVoice` means "the author is in no voice channel", which is
        // a different thing from "in a channel we cannot join". Rendering it
        // as a denial would tell someone to grant a permission that is
        // already granted.
        assert_eq!(p.voice, VoiceCheck::NotInVoice);
        assert!(p.voice.perms().is_none());
        assert!(p.text.is_whole());
    }

    #[test]
    fn an_unseeable_voice_channel_is_not_the_same_state_as_no_voice_channel() {
        let unreadable = compute(text_ch(), TEXT_REQUIRED, VoiceCheck::Unreadable(voice_ch()));
        let absent = compute(text_ch(), TEXT_REQUIRED, VoiceCheck::NotInVoice);
        // 🪤 Both have no `VoicePerms`, which is exactly how the two-state
        // version collapsed them into one and told a user sitting in a voice
        // channel that they were not in one. `perms()` returning `None` must
        // never be read as "no voice channel".
        assert!(unreadable.voice.perms().is_none());
        assert!(absent.voice.perms().is_none());
        assert_ne!(unreadable.voice, absent.voice);
        assert_eq!(unreadable.voice, VoiceCheck::Unreadable(voice_ch()));
    }

    #[test]
    fn a_channel_we_cannot_see_earns_a_view_channel_refusal_and_names_nothing_else() {
        let err = unreadable_refusal(voice_ch());
        match &err {
            CrackedError::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                assert_eq!(*scope, PermScope::Voice);
                assert_eq!(*channel, voice_ch().widen());
                assert_eq!(*missing, Permissions::VIEW_CHANNEL);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
        let rendered = format!("{err}");
        assert!(rendered.contains("View Channel"), "got {rendered}");
        assert!(
            rendered.contains(&format!("<#{}>", voice_ch())),
            "must name the channel: {rendered}"
        );
        // 🪤 CONNECT and SPEAK are *unknown* here, not known-missing. Naming
        // them would send an admin to grant permissions they already granted.
        assert!(
            !rendered.contains("Connect"),
            "Connect is unknown, not missing: {rendered}"
        );
        assert!(
            !rendered.contains("Speak"),
            "Speak is unknown, not missing: {rendered}"
        );
    }

    // `classify_voice` is where the three-way distinction is actually made.
    // `compute` only carries a `VoiceCheck` someone else decided, so testing
    // `compute` proves nothing about whether `resolve` ever produces
    // `Unreadable`. These are the tests that pin that.

    #[test]
    fn no_voice_channel_classifies_as_not_in_voice() {
        let got = classify_voice(
            None,
            |_| panic!("nothing to look up"),
            |_| panic!("nothing to look up"),
        );
        assert_eq!(got, VoiceCheck::NotInVoice);
    }

    #[test]
    fn a_channel_the_guild_does_not_list_classifies_as_unreadable() {
        // 🪤 THE regression. Discord omits channels the bot lacks
        // VIEW_CHANNEL on from GUILD_CREATE, so an author sitting in one
        // leaves the guild's channel list without an entry. Classifying that
        // as `NotInVoice` tells them they are not in a voice channel -- false
        // and unactionable -- and lets the gate wave the join through into
        // songbird's ~10s timeout.
        let got = classify_voice(Some(voice_ch()), |_| false, |_| VOICE_REQUIRED);
        assert_eq!(
            got,
            VoiceCheck::Unreadable(voice_ch()),
            "a channel we cannot see is not the same as no channel"
        );
        assert_ne!(
            got,
            VoiceCheck::NotInVoice,
            "collapsing these two is the bug this state exists to prevent"
        );
        match got {
            VoiceCheck::Unreadable(cid) => assert_eq!(cid, voice_ch(), "the id must survive"),
            other => panic!("expected Unreadable, got {other:?}"),
        }
    }

    #[test]
    fn a_channel_the_guild_lists_classifies_as_resolved_carrying_its_bitset() {
        let granted = VOICE_REQUIRED - Permissions::SPEAK;
        let got = classify_voice(Some(voice_ch()), |_| true, |_| granted);
        let v = got.perms().expect("a known channel resolves");
        assert_eq!(v.channel, voice_ch());
        // Bit-exact: a bitset mangled on the way through would name the wrong
        // permission in the refusal.
        assert_eq!(v.granted, granted);
        assert_eq!(v.missing(), Permissions::SPEAK);
    }

    #[test]
    fn granted_in_is_not_consulted_for_a_channel_we_cannot_see() {
        // 🪤 Order matters. If `granted_in` were consulted first, a caller
        // whose lookup falls back to `Permissions::empty()` for an absent
        // channel would produce `Resolved(empty)` -- a refusal naming Connect
        // and Speak instead of View Channel, sending an admin to grant two
        // permissions that are very likely already granted.
        let got = classify_voice(
            Some(voice_ch()),
            |_| false,
            |_| panic!("granted_in must not be asked about a channel we cannot see"),
        );
        assert_eq!(got, VoiceCheck::Unreadable(voice_ch()));
    }

    #[test]
    fn an_unreadable_guild_classifies_a_join_as_unknown() {
        let got = classify_join(
            voice_ch(),
            false,
            |_| panic!("nothing to look up"),
            |_| panic!("nothing to look up"),
        );
        assert_eq!(got, JoinLookup::Unknown, "fail open on genuine ignorance");
    }

    #[test]
    fn a_join_target_the_guild_does_not_list_classifies_as_withheld() {
        // 🪤 The gate half of the same regression: this arm returning
        // `Unknown` is `Ok(())`, and the join proceeds to songbird's ~10s
        // `JoinError::TimedOut`, which names nothing.
        let got = classify_join(voice_ch(), true, |_| false, |_| VOICE_REQUIRED);
        assert_eq!(got, JoinLookup::Withheld);
        assert_ne!(got, JoinLookup::Unknown, "this is information, not a gap");
    }

    #[test]
    fn a_join_target_the_guild_lists_classifies_as_granted_bit_exactly() {
        let granted = VOICE_REQUIRED - Permissions::CONNECT;
        let got = classify_join(voice_ch(), true, |_| true, |_| granted);
        assert_eq!(got, JoinLookup::Granted(granted));
    }

    #[test]
    fn a_role_the_cache_does_not_have_makes_the_bitset_untrustworthy() {
        let gid = serenity::all::GuildId::new(10);
        let everyone = RoleId::new(10);
        let extra = RoleId::new(11);
        let known = [everyone, extra];

        assert!(
            roles_resolvable(gid, &[extra], |r| known.contains(&r)),
            "every role resolves, so the bitset can be trusted"
        );
        // 🪤 serenity substitutes Permissions::empty() for a role it cannot
        // find and only logs, so the bitset comes back *under*-reported and
        // the gate would refuse a join that works.
        assert!(
            !roles_resolvable(gid, &[RoleId::new(99)], |r| known.contains(&r)),
            "an unknown bot role must not be silently treated as no permissions"
        );
    }

    #[test]
    fn a_missing_everyone_role_makes_the_bitset_untrustworthy() {
        let gid = serenity::all::GuildId::new(10);
        // @everyone carries the guild's baseline grants and its id is the
        // guild id; without it every permission looks denied.
        assert!(!roles_resolvable(gid, &[], |_| false));
        assert!(roles_resolvable(gid, &[], |r| r == RoleId::new(10)));
    }

    #[test]
    fn everything_granted_earns_no_refusal() {
        assert!(voice_refusal(voice_ch(), VOICE_REQUIRED).is_none());
    }

    #[test]
    fn missing_speak_only_is_named_bit_exactly() {
        let granted = VOICE_REQUIRED - Permissions::SPEAK;
        let err = voice_refusal(voice_ch(), granted).expect("SPEAK is missing");
        match err {
            CrackedError::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                assert_eq!(scope, PermScope::Voice);
                assert_eq!(channel, voice_ch().widen());
                assert_eq!(missing, Permissions::SPEAK);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
    }

    #[test]
    fn missing_connect_only_is_named_bit_exactly() {
        let granted = VOICE_REQUIRED - Permissions::CONNECT;
        let err = voice_refusal(voice_ch(), granted).expect("CONNECT is missing");
        match err {
            CrackedError::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                assert_eq!(scope, PermScope::Voice);
                assert_eq!(channel, voice_ch().widen());
                assert_eq!(missing, Permissions::CONNECT);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
    }

    #[test]
    fn both_missing_are_named_together_and_the_channel_is_in_the_message() {
        let err = voice_refusal(voice_ch(), Permissions::empty()).expect("both perms are missing");
        match &err {
            CrackedError::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                assert_eq!(*scope, PermScope::Voice);
                assert_eq!(*channel, voice_ch().widen());
                assert_eq!(*missing, VOICE_REQUIRED);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
        let rendered = format!("{err}");
        assert!(rendered.contains("Connect"), "got {rendered}");
        assert!(rendered.contains("Speak"), "got {rendered}");
        assert!(
            rendered.contains(&format!("<#{}>", voice_ch())),
            "must name the channel: {rendered}"
        );
    }

    // `resolve` and `ensure_can_join` read a live serenity Cache, which cannot
    // be constructed meaningfully offline, so their *logic* is tested through
    // `voice_refusal` above and their *placement* by the source-scan guard in
    // Task 4. What is tested here is the one decision that is neither:
    // what they do when the cache cannot answer.

    #[test]
    fn an_unknown_lookup_refuses_nothing() {
        // 🪤 Fail OPEN. Refusing a join we merely could not verify would
        // break working guilds during the cache warm-up after every restart.
        assert!(join_refusal(voice_ch(), JoinLookup::Unknown).is_none());
    }

    #[test]
    fn a_withheld_channel_refuses_the_join_instead_of_waving_it_through() {
        // 🪤 This is the C1 regression. The two-state version answered
        // `Ok(())` here, so the join went ahead and songbird spent ~10s
        // arriving at `JoinError::TimedOut`, which names nothing -- the exact
        // symptom this module exists to delete.
        let err = join_refusal(voice_ch(), JoinLookup::Withheld)
            .expect("a channel Discord withheld must not be waved through");
        match err {
            CrackedError::MissingBotPermissions { missing, .. } => {
                assert_eq!(missing, Permissions::VIEW_CHANNEL);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
    }

    #[test]
    fn a_granted_lookup_defers_to_the_voice_bitset() {
        assert!(join_refusal(voice_ch(), JoinLookup::Granted(VOICE_REQUIRED)).is_none());
        let err = join_refusal(
            voice_ch(),
            JoinLookup::Granted(VOICE_REQUIRED - Permissions::CONNECT),
        )
        .expect("CONNECT is missing");
        match err {
            CrackedError::MissingBotPermissions { missing, .. } => {
                assert_eq!(missing, Permissions::CONNECT);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
    }

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
/// A first version of this module still got that shape wrong one level up:
/// it derived the join sites *within* a file, but the file list itself was
/// three `include_str!` literals, hand-chosen the same way the old play-
/// history count was. A join added in a fourth *file* would never be
/// scanned, and the test would pass silently. So this walks `src/` at test
/// time instead — the file set is discovered, not listed. Adding a fourth
/// join site anywhere in `crack-core` fails this test until it is gated too.
///
/// 🪤 Anywhere in `crack-core`, and nowhere else. `crack-types` also depends
/// on songbird, so a join added there is NOT guarded by this. The walk is
/// not simply widened to the workspace root because the matcher below is not
/// songbird-aware: `crack-sleevenote/src/client.rs` calls
/// `self.join(&["health"])` on a URL builder, whose first argument is not a
/// string literal either, and every such call would fail this test. Widening
/// the walk means teaching the matcher what a songbird join looks like
/// first.
#[cfg(test)]
mod join_site_guard_tests {
    use std::path::{Path, PathBuf};

    /// The call that must precede every join.
    const GATE: &str = "ensure_can_join(";

    /// How far back from a join to look for its gate. Generous enough to span
    /// a `let Some(..) = ... else` or a log line in between.
    ///
    /// 🪤 It does NOT pair a gate with its own join. Any two joins within
    /// 1200 bytes of each other share a window, so an earlier join's gate
    /// satisfies a later one. What this catches is an *ungated* join, not a
    /// mis-paired one -- and no window size fixes that, only parsing would.
    const WINDOW: usize = 1200;

    /// This file, relative to `src/`. Excluded from the walk below: it
    /// contains the literal `GATE` string (and this very sentence), so
    /// scanning it would let it satisfy its own check no matter what the
    /// rest of the tree looks like. The three-file hand-list this replaced
    /// happened to be safe from that trap by omission, not by design --
    /// this exclusion makes it deliberate.
    const SELF_PATH: &str = "music/perms.rs";

    /// Every `.rs` file under `src/`, found by walking the tree rather than
    /// naming files up front -- a hard-coded list is the exact defect this
    /// test exists to prevent, moved up one level.
    fn all_source_files() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        walk(&root, &root, &mut out);
        out
    }

    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                if rel != Path::new(SELF_PATH) {
                    out.push(path);
                }
            }
        }
    }

    /// Round a raw byte offset down to a char boundary.
    ///
    /// 🪤 `at - WINDOW` is arithmetic on bytes and can land inside a
    /// multi-byte character. This repo puts 🪤, ⚠️ and ❌ in comments
    /// routinely, so that is a question of when, not whether. Slicing there
    /// panics with `str`'s byte-index error, which replaces the report this
    /// test exists to produce with noise about UTF-8.
    fn floor_boundary(src: &str, mut i: usize) -> usize {
        while i > 0 && !src.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

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
    fn a_window_edge_inside_a_multibyte_character_reports_instead_of_panicking() {
        // 🪤 The window edge is byte arithmetic, and this repo puts 🪤, ⚠️
        // and ❌ in comments routinely, so it lands mid-character sooner or
        // later. A raw slice there panics with `str`'s byte-index error,
        // replacing the report this guard exists to print with noise about
        // UTF-8 -- the test fails, but for the wrong reason and with the
        // wrong message.
        let src = "🪤 ensure_can_join( .join(x)";
        for i in 1..4 {
            assert!(!src.is_char_boundary(i), "byte {i} is inside the trap");
            assert_eq!(floor_boundary(src, i), 0);
            // The point: this does not panic.
            let _ = &src[floor_boundary(src, i)..];
        }
        // A boundary is left exactly where it is.
        assert_eq!(floor_boundary(src, 4), 4);
        assert_eq!(floor_boundary(src, 0), 0);
    }

    #[test]
    fn every_songbird_join_is_preceded_by_the_gate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let files = all_source_files();
        assert!(
            !files.is_empty(),
            "the walk from {} found zero .rs files -- the scan has stopped \
             checking, not that there is nothing left to check. Fix the walk \
             before trusting this test again.",
            root.display()
        );

        let mut checked = 0usize;
        for path in &files {
            let rel = path.strip_prefix(&root).unwrap_or(path);
            let src = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            for at in songbird_join_offsets(&src) {
                checked += 1;
                let start = floor_boundary(&src, at.saturating_sub(WINDOW));
                let before = &src[start..at];
                assert!(
                    before.contains(GATE),
                    "{}: a songbird join at byte {at} is not preceded by \
                     `{GATE}` within {WINDOW} bytes.\n\n\
                     Every join must be gated, or a guild missing CONNECT or \
                     SPEAK gets songbird's ~10s JoinError::TimedOut, which \
                     names nothing. Add the gate rather than widening this \
                     test.\n\n\
                     Context:\n{}",
                    rel.display(),
                    &src[floor_boundary(&src, start.max(at.saturating_sub(300)))..at]
                );
            }
        }
        assert!(
            checked >= 3,
            "expected at least the 3 known songbird join sites, scanned \
             {checked} across {} files -- the scan found less than it \
             should, which means it has stopped checking rather than that \
             the joins are gone. Fix the scan.",
            files.len()
        );
    }
}
