//! Who may see a guild's queue, and who may change it.
//!
//! The decision is a pure function over a [`Presence`], as `music::perms`
//! does it: the cache and HTTP lookups that build a `Presence` are thin
//! glue, and every branch of the decision is tested without either.

use dashmap::DashMap;
use serenity::all::{Cache, ChannelId, GuildId, Http, UserId};
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

/// Build a [`Presence`]: the cache, then the memo, then one HTTP lookup.
pub async fn presence(
    cache: &Cache,
    http: &Http,
    memo: &MemberMemo,
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
        let status = match http.get_member(g, u).await {
            Ok(_) => Ok(()),
            Err(serenity::Error::Http(e)) => Err(e.status_code().map(|s| s.as_u16())),
            Err(_) => Err(None),
        };
        let lookup = lookup_from_status(status);
        memo.record(g, u, &lookup, Instant::now());
        match lookup {
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
        let got = presence(&cache, &http, &memo, G, U, Some(A)).await;
        assert_eq!(got.membership, Membership::NotMember);
    }
}
