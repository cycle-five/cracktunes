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

/// Permissions a voice join needs. Missing any of them blocks it outright.
///
/// 🪤 `VIEW_CHANNEL` is in here, not only in [`TEXT_REQUIRED`]. It is required
/// to *initiate* a connection, which is the only way a bot ever gets into a
/// voice channel: it sends a voice state update over the gateway, and Discord
/// validates that against VIEW_CHANNEL as well as CONNECT, then silently
/// drops it.
///
/// Note this is NOT the same as "cannot be present in". Hiding a voice
/// channel while granting CONNECT is a deliberate pattern -- members who
/// cannot see it can still be dragged in by someone with MOVE_MEMBERS and
/// then talk normally. That escape hatch does not exist for a bot: there is
/// nothing to drag until it is already in voice, and it cannot get there by
/// itself. So for our purposes the channel is unreachable, and the refusal
/// names the permission that would actually fix it.
///
/// Classified as text-only degradation ("I can play, I just cannot announce")
/// until production proved otherwise: SHAMELESS 21+, 2026-09-12, where
/// Discord reported CONNECT: YES, SPEAK: YES, VIEW_CHANNEL: NO and the join
/// died in songbird's ~10s timeout while this gate said everything was fine.
pub const VOICE_REQUIRED: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::CONNECT)
    .union(Permissions::SPEAK);

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

/// Proof that [`ensure_can_join`] said yes, for one specific guild and
/// channel.
///
/// The fields are private and this module holds the only constructor, so the
/// sole way to obtain one is to have passed the gate.
/// `music_utils::join_permitted` -- the crate's only caller of
/// `Songbird::join` -- takes one **by value**. A join that skipped the
/// permission check therefore does not compile.
///
/// 🔑 This replaces `join_site_guard_tests`, which read the join sites as text
/// and asserted `ensure_can_join(` appeared within 1200 bytes before
/// `manager.join`. That guard could not pair a gate with its own join (any two
/// joins inside one window satisfied each other), broke four separate times on
/// ordinary prose landing in the window, and was in the end a linter written
/// in a test. The compiler does the same job exactly, at every site, with no
/// window and no false positives.
#[derive(Debug)]
#[must_use = "a JoinPermit that is never spent means the join never happened"]
pub struct JoinPermit {
    guild_id: GuildId,
    channel_id: ChannelId,
}

impl JoinPermit {
    /// The guild and channel this permit authorises.
    ///
    /// Consuming, because a permit authorises exactly one join: spending it is
    /// what stops a single permission check from covering a later join into
    /// somewhere else.
    pub fn into_parts(self) -> (GuildId, ChannelId) {
        (self.guild_id, self.channel_id)
    }
}

/// The blocking gate: refuse a join Discord would silently drop.
///
/// A thin cache layer over [`join_refusal`], which holds the actual rule.
///
/// Returns a [`JoinPermit`] rather than `()` so that the ordering this gate
/// depends on is enforced by the type system instead of by convention -- see
/// that type for why.
pub fn ensure_can_join(
    cache: &Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<JoinPermit, CrackedError> {
    match join_refusal(channel_id, join_lookup(cache, guild_id, channel_id)) {
        Some(err) => Err(err),
        None => Ok(JoinPermit {
            guild_id,
            channel_id,
        }),
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

    /// 🪤 Written with literal permissions rather than `VOICE_REQUIRED - X`.
    /// Every other refusal test derives its input from the constant, so they
    /// move with it and cannot notice it being wrong -- all 36 passed both
    /// before and after `VIEW_CHANNEL` was added to it. A test defined in
    /// terms of the value under test checks an identity, not a fact.
    #[test]
    fn connect_and_speak_without_view_channel_still_blocks_the_join() {
        let granted = Permissions::CONNECT.union(Permissions::SPEAK);
        let err = voice_refusal(voice_ch(), granted).expect("VIEW_CHANNEL is missing");
        match err {
            CrackedError::MissingBotPermissions {
                scope,
                channel,
                missing,
            } => {
                assert_eq!(scope, PermScope::Voice);
                assert_eq!(channel, voice_ch().widen());
                assert_eq!(missing, Permissions::VIEW_CHANNEL);
            },
            other => panic!("expected MissingBotPermissions, got {other:?}"),
        }
    }

    /// The other direction, also spelled out, so the pair pins the exact
    /// boundary Discord enforces rather than the one we happen to declare.
    #[test]
    fn view_channel_connect_and_speak_together_earn_no_refusal() {
        let granted = Permissions::VIEW_CHANNEL
            .union(Permissions::CONNECT)
            .union(Permissions::SPEAK);
        assert!(voice_refusal(voice_ch(), granted).is_none());
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
