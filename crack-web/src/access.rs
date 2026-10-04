//! Who may see a guild's queue, and who may change it.
//!
//! The decision is a pure function over a [`Presence`], as `music::perms`
//! does it: the cache and HTTP lookups that build a `Presence` are thin
//! glue, and every branch of the decision is tested without either.

use dashmap::DashMap;
use serenity::all::{Cache, ChannelId, GuildId, Http, Permissions, RoleId, UserId};
use std::time::{Duration, Instant};

/// How long an HTTP membership answer is trusted.
pub const MEMBER_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    Member,
    NotMember,
    /// Discord did not answer; we cannot say.
    Unknown,
}

/// Everything the access decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct Presence {
    pub membership: Membership,
    pub user_channel: Option<ChannelId>,
    pub bot_channel: Option<ChannelId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Not a member: answer 404, so a guild id confirms nothing.
    Hidden,
    /// Could not tell: answer 503.
    Unavailable,
    View,
    /// In the bot's voice channel: may reorder.
    Control,
}

/// The whole rule. Viewing needs membership; controlling needs the user's
/// voice channel to be the bot's.
pub fn decide(p: &Presence) -> Access {
    match p.membership {
        Membership::NotMember => Access::Hidden,
        Membership::Unknown => Access::Unavailable,
        Membership::Member => match (p.user_channel, p.bot_channel) {
            (Some(user), Some(bot)) if user == bot => Access::Control,
            _ => Access::View,
        },
    }
}

/// Who may see a guild's queue history (spec: dashboard history design §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAccess {
    /// Not a member: 404, as for the queue.
    Hidden,
    /// Discord did not answer: 503.
    Unavailable,
    /// A member without Manage Server: 403.
    Forbidden,
    Allowed,
}

/// Manage Server, as Discord computes it at the guild level: the owner always;
/// otherwise `@everyone`'s permissions plus each of the member's roles',
/// with Administrator implying everything. Roles the guild does not have
/// count for nothing. Channel overrides do not apply to a guild permission.
pub fn has_manage_guild(
    owner: UserId,
    user: UserId,
    everyone: Permissions,
    roles: &[RoleId],
    role_permissions: impl Fn(RoleId) -> Option<Permissions>,
) -> bool {
    if user == owner {
        return true;
    }
    let perms = roles
        .iter()
        .filter_map(|r| role_permissions(*r))
        .fold(everyone, |acc, p| acc | p);
    perms.intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD)
}

/// The history rule. A bot owner sees every server's history, member or not,
/// for debugging and support. `manages` is `None` when the member's roles
/// could not be read.
pub fn decide_history(owner: bool, membership: Membership, manages: Option<bool>) -> HistoryAccess {
    if owner {
        return HistoryAccess::Allowed;
    }
    match (membership, manages) {
        (Membership::NotMember, _) => HistoryAccess::Hidden,
        (Membership::Unknown, _) | (Membership::Member, None) => HistoryAccess::Unavailable,
        (Membership::Member, Some(false)) => HistoryAccess::Forbidden,
        (Membership::Member, Some(true)) => HistoryAccess::Allowed,
    }
}

/// Remembered role lists of members fetched over HTTP, so a history page
/// polling every 10 s asks Discord at most once per TTL.
pub struct RoleMemo {
    ttl: Duration,
    entries: DashMap<(GuildId, UserId), (Vec<RoleId>, Instant)>,
}

impl RoleMemo {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: DashMap::new(),
        }
    }

    pub fn get(&self, g: GuildId, u: UserId, now: Instant) -> Option<Vec<RoleId>> {
        let entry = self.entries.get(&(g, u))?;
        let (roles, at) = &*entry;
        (now.saturating_duration_since(*at) < self.ttl).then(|| roles.clone())
    }

    pub fn record(&self, g: GuildId, u: UserId, roles: Vec<RoleId>, now: Instant) {
        self.entries.insert((g, u), (roles, now));
    }
}

/// The outcome of asking Discord whether a user is a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Member,
    NotMember,
    /// Anything but success or 404: a 5xx, a 429, a timeout.
    Failed,
}

/// `Ok` is membership; only a 404 is a definite "no".
pub fn lookup_from_status(result: Result<(), Option<u16>>) -> Lookup {
    match result {
        Ok(()) => Lookup::Member,
        Err(Some(404)) => Lookup::NotMember,
        Err(_) => Lookup::Failed,
    }
}

/// Remembered HTTP membership answers. Failures are never remembered.
pub struct MemberMemo {
    ttl: Duration,
    entries: DashMap<(GuildId, UserId), (bool, Instant)>,
}

impl MemberMemo {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: DashMap::new(),
        }
    }

    pub fn get(&self, g: GuildId, u: UserId, now: Instant) -> Option<bool> {
        let entry = self.entries.get(&(g, u))?;
        let (member, at) = *entry;
        (now.saturating_duration_since(at) < self.ttl).then_some(member)
    }

    pub fn record(&self, g: GuildId, u: UserId, lookup: &Lookup, now: Instant) {
        match lookup {
            Lookup::Member => {
                self.entries.insert((g, u), (true, now));
            },
            Lookup::NotMember => {
                self.entries.insert((g, u), (false, now));
            },
            Lookup::Failed => {},
        }
    }
}

/// Record one HTTP member lookup: the membership answer in `members` and, when
/// Discord returned the member, its role ids in `roles` at the same instant.
/// The history check that follows then reads the roles from the memo instead
/// of making a second call (spec: dashboard history design §1).
pub fn record_member_lookup(
    members: &MemberMemo,
    roles: &RoleMemo,
    g: GuildId,
    u: UserId,
    result: Result<Vec<RoleId>, Option<u16>>,
    now: Instant,
) -> Lookup {
    let (status, member_roles) = match result {
        Ok(r) => (Ok(()), Some(r)),
        Err(e) => (Err(e), None),
    };
    let lookup = lookup_from_status(status);
    members.record(g, u, &lookup, now);
    if let Some(r) = member_roles {
        roles.record(g, u, r, now);
    }
    lookup
}

/// What the cache alone can say.
#[derive(Debug, Clone, Copy)]
pub struct CachedPresence {
    /// The bot is in this guild (it is cached).
    pub guild_known: bool,
    /// In the cached member list, or has a voice state here. The member list
    /// of a large guild is partial -- the bot never requests member chunks --
    /// so `false` here is not "not a member".
    pub member: bool,
    pub user_channel: Option<ChannelId>,
}

pub fn cached_presence(cache: &Cache, g: GuildId, u: UserId) -> CachedPresence {
    let Some(guild) = cache.guild(g) else {
        return CachedPresence {
            guild_known: false,
            member: false,
            user_channel: None,
        };
    };
    let voice = guild.voice_states.get(&u);
    CachedPresence {
        guild_known: true,
        member: guild.members.get(&u).is_some() || voice.is_some(),
        user_channel: voice.and_then(|v| v.channel_id),
    }
}

/// Build a [`Presence`]: the cache, then the memo, then one HTTP lookup, whose
/// member's roles go into `roles` for the history check.
pub async fn presence(
    cache: &Cache,
    http: &Http,
    memo: &MemberMemo,
    roles: &RoleMemo,
    g: GuildId,
    u: UserId,
    bot_channel: Option<ChannelId>,
) -> Presence {
    let cached = cached_presence(cache, g, u);
    let membership = if !cached.guild_known {
        Membership::NotMember
    } else if cached.member {
        Membership::Member
    } else if let Some(known) = memo.get(g, u, Instant::now()) {
        if known {
            Membership::Member
        } else {
            Membership::NotMember
        }
    } else {
        let result = match http.get_member(g, u).await {
            Ok(m) => Ok(m.roles.iter().copied().collect()),
            Err(serenity::Error::Http(e)) => Err(e.status_code().map(|s| s.as_u16())),
            Err(_) => Err(None),
        };
        match record_member_lookup(memo, roles, g, u, result, Instant::now()) {
            Lookup::Member => Membership::Member,
            Lookup::NotMember => Membership::NotMember,
            Lookup::Failed => Membership::Unknown,
        }
    };
    Presence {
        membership,
        user_channel: cached.user_channel,
        bot_channel,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::{Duration, Instant};

    const A: ChannelId = ChannelId::new(10);
    const B: ChannelId = ChannelId::new(11);
    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(2);

    fn p(membership: Membership, user: Option<ChannelId>, bot: Option<ChannelId>) -> Presence {
        Presence {
            membership,
            user_channel: user,
            bot_channel: bot,
        }
    }

    #[test]
    fn the_decision_table() {
        use Membership::*;
        let cases = [
            (p(NotMember, Some(A), Some(A)), Access::Hidden),
            (p(Unknown, None, Some(A)), Access::Unavailable),
            (p(Member, None, Some(A)), Access::View),
            (p(Member, Some(B), Some(A)), Access::View),
            (p(Member, Some(A), None), Access::View),
            (p(Member, None, None), Access::View),
            (p(Member, Some(A), Some(A)), Access::Control),
        ];
        for (presence, want) in cases {
            assert_eq!(decide(&presence), want, "{presence:?}");
        }
    }

    #[test]
    fn only_a_404_means_not_a_member() {
        assert_eq!(lookup_from_status(Ok(())), Lookup::Member);
        assert_eq!(lookup_from_status(Err(Some(404))), Lookup::NotMember);
        assert_eq!(lookup_from_status(Err(Some(500))), Lookup::Failed);
        assert_eq!(lookup_from_status(Err(Some(429))), Lookup::Failed);
        assert_eq!(lookup_from_status(Err(None)), Lookup::Failed);
    }

    #[test]
    fn the_memo_remembers_answers_not_failures_and_forgets_in_time() {
        let memo = MemberMemo::new(Duration::from_secs(300));
        let t0 = Instant::now();
        memo.record(G, U, &Lookup::Failed, t0);
        assert_eq!(memo.get(G, U, t0), None, "a failure is not remembered");
        memo.record(G, U, &Lookup::NotMember, t0);
        assert_eq!(memo.get(G, U, t0 + Duration::from_secs(299)), Some(false));
        assert_eq!(
            memo.get(G, U, t0 + Duration::from_secs(301)),
            None,
            "expired"
        );
        memo.record(G, U, &Lookup::Member, t0);
        assert_eq!(memo.get(G, U, t0), Some(true));
        assert_eq!(memo.get(G, UserId::new(3), t0), None, "keyed by user");
    }

    #[test]
    fn one_member_lookup_fills_both_memos_at_once() {
        let members = MemberMemo::new(MEMBER_TTL);
        let roles = RoleMemo::new(MEMBER_TTL);
        let t0 = Instant::now();
        let got = record_member_lookup(&members, &roles, G, U, Ok(vec![RoleId::new(5)]), t0);
        assert_eq!(got, Lookup::Member);
        assert_eq!(members.get(G, U, t0), Some(true));
        assert_eq!(
            roles.get(G, U, t0),
            Some(vec![RoleId::new(5)]),
            "the history check needs no second call"
        );
        let late = t0 + MEMBER_TTL;
        assert_eq!(
            (roles.get(G, U, late), members.get(G, U, late)),
            (None, None),
            "recorded at the same instant, so both expire together"
        );

        let other = UserId::new(3);
        let got = record_member_lookup(&members, &roles, G, other, Err(Some(404)), t0);
        assert_eq!(got, Lookup::NotMember);
        assert_eq!(members.get(G, other, t0), Some(false));
        assert_eq!(roles.get(G, other, t0), None, "no member, no roles");

        let third = UserId::new(4);
        let got = record_member_lookup(&members, &roles, G, third, Err(Some(503)), t0);
        assert_eq!(got, Lookup::Failed);
        assert_eq!(
            (members.get(G, third, t0), roles.get(G, third, t0)),
            (None, None)
        );
    }

    #[test]
    fn an_uncached_guild_is_not_known() {
        let cache = serenity::all::Cache::new();
        let c = cached_presence(&cache, G, U);
        assert!(!c.guild_known);
        assert!(!c.member);
        assert_eq!(c.user_channel, None);
    }

    #[tokio::test]
    async fn a_guild_the_bot_is_not_in_is_not_a_membership_question() {
        // No HTTP is made: the token is garbage and no server is reachable,
        // so a lookup would come back `Failed` and read `Unknown`.
        let cache = serenity::all::Cache::new();
        let http = serenity::all::Http::new(crack_types::get_valid_token());
        let memo = MemberMemo::new(MEMBER_TTL);
        let roles = RoleMemo::new(MEMBER_TTL);
        let got = presence(&cache, &http, &memo, &roles, G, U, Some(A)).await;
        assert_eq!(got.membership, Membership::NotMember);
    }

    use serenity::all::{Permissions, RoleId};

    #[test]
    fn manage_guild_comes_from_owner_admin_or_the_permission() {
        let owner = UserId::new(1);
        let me = UserId::new(2);
        let admin = RoleId::new(10);
        let manager = RoleId::new(11);
        let dj = RoleId::new(12);
        let perms = |r: RoleId| match r.get() {
            10 => Some(Permissions::ADMINISTRATOR),
            11 => Some(Permissions::MANAGE_GUILD),
            12 => Some(Permissions::CONNECT | Permissions::SPEAK),
            _ => None,
        };
        let none = Permissions::empty();
        assert!(
            has_manage_guild(owner, owner, none, &[], perms),
            "the owner"
        );
        assert!(
            has_manage_guild(owner, me, none, &[admin], perms),
            "Administrator"
        );
        assert!(
            has_manage_guild(owner, me, none, &[dj, manager], perms),
            "Manage Server on a role"
        );
        assert!(
            has_manage_guild(owner, me, Permissions::MANAGE_GUILD, &[], perms),
            "Manage Server on @everyone"
        );
        assert!(
            !has_manage_guild(owner, me, none, &[dj], perms),
            "no permission"
        );
        assert!(
            !has_manage_guild(owner, me, none, &[RoleId::new(99)], perms),
            "a role the guild does not have counts for nothing"
        );
    }

    #[test]
    fn the_history_decision_table() {
        use Membership::*;
        assert_eq!(
            decide_history(false, NotMember, Some(true)),
            HistoryAccess::Hidden
        );
        assert_eq!(
            decide_history(false, Unknown, Some(true)),
            HistoryAccess::Unavailable
        );
        assert_eq!(
            decide_history(false, Member, None),
            HistoryAccess::Unavailable
        );
        assert_eq!(
            decide_history(false, Member, Some(false)),
            HistoryAccess::Forbidden
        );
        assert_eq!(
            decide_history(false, Member, Some(true)),
            HistoryAccess::Allowed
        );
    }

    /// Bot owners see any server's history, for debugging and support:
    /// whether or not they are members, have Manage Server, or Discord answered.
    #[test]
    fn a_bot_owner_sees_every_servers_history() {
        use Membership::*;
        for membership in [NotMember, Unknown, Member] {
            for manages in [None, Some(false), Some(true)] {
                assert_eq!(
                    decide_history(true, membership, manages),
                    HistoryAccess::Allowed,
                    "{membership:?} {manages:?}"
                );
            }
        }
    }

    #[test]
    fn the_role_memo_remembers_roles_and_forgets_in_time() {
        let memo = RoleMemo::new(Duration::from_secs(300));
        let (g, u) = (GuildId::new(1), UserId::new(2));
        let t0 = Instant::now();
        assert_eq!(memo.get(g, u, t0), None);
        memo.record(g, u, vec![RoleId::new(5)], t0);
        assert_eq!(
            memo.get(g, u, t0 + Duration::from_secs(299)),
            Some(vec![RoleId::new(5)])
        );
        assert_eq!(memo.get(g, u, t0 + Duration::from_secs(300)), None);
    }
}
