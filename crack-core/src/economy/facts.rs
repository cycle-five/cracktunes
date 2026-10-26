//! Target facts gathered from the cache, rooms' guard and the database: the
//! glue around `target.rs`'s pure rules, kept in crack-core because it reads
//! `Data` and rooms.

use super::settings::ActionKind;
use super::shop::BuyError;
use super::target::{
    cached_position, is_owner, member_facts, top_position, voice_flags, voice_of, TargetFacts,
};
use super::wallet::ActionState;
use crate::db::economy::{self as economy, ActionRow};
use crate::rooms::guard::{member_can_join, BaselineView};
use crate::rooms::state::StoredOverwrite;
use crate::Data;
use chrono::{DateTime, Utc};
use crack_common::perms::roles_resolvable;
use serenity::all::{Cache, ChannelId, GuildId, Member, Permissions, RoleId, UserId};
use std::collections::HashMap;

/// Whether `member` could join `channel` on their own, by Discord's
/// algorithm over the channel's overwrites. `None` when the cache cannot
/// answer (guild, member, channel or one of their roles not cached).
pub fn can_join(cache: &Cache, guild: GuildId, member: UserId, channel: ChannelId) -> Option<bool> {
    let g = cache.guild(guild)?;
    let m = g.members.get(&member)?;
    if !roles_resolvable(guild, &m.roles, |r| g.roles.contains_key(&r)) {
        return None;
    }
    let ch = g.channels.get(&channel)?;
    let everyone = g.roles.get(&RoleId::new(guild.get()))?.permissions;
    let role_perms: HashMap<RoleId, Permissions> =
        g.roles.iter().map(|r| (r.id, r.permissions)).collect();
    let overwrites: Vec<StoredOverwrite> = ch
        .permission_overwrites
        .iter()
        .map(StoredOverwrite::from)
        .collect();
    let view = BaselineView {
        guild,
        everyone,
        role_perms: &role_perms,
        overwrites: &overwrites,
    };
    Some(member_can_join(&view, member, &m.roles))
}

/// The end of a running (`active`, not winding down) prank of these kinds on
/// `member`.
fn running_end(rows: &[ActionRow]) -> Option<DateTime<Utc>> {
    rows.iter()
        .filter(|r| r.state == ActionState::Active)
        .find_map(|r| r.expires_at)
}

/// Who [`gather_facts`] describes.
#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    /// The member as Discord resolved them: the interaction's, or fetched.
    pub member: &'a Member,
    /// The member is the buyer.
    pub is_self: bool,
}

/// Everything [`check_target`](super::target::check_target) needs about a member. Their roles, nickname
/// and shown name come from the [`Member`] passed in (Discord resolves it in
/// the interaction), placed by the cached server roles; their voice state
/// from the cache; opt-out, immunity, mirror and running rows from the
/// database. Used for the target of a prank and, the same way, for the buyer
/// (so a mirror can bounce a prank onto them).
///
/// The gaps fail closed: a role the cache cannot place leaves
/// `top_role_position` `None` (refused, can't tell), and a drag with no
/// answer for the destination reads `None` (refused).
pub async fn gather_facts(
    data: &Data,
    cache: &Cache,
    guild: GuildId,
    who: Subject<'_>,
    kind: ActionKind,
    buyer_voice: Option<ChannelId>,
    now: DateTime<Utc>,
) -> Result<TargetFacts, BuyError> {
    let Subject {
        member: resolved,
        is_self,
    } = who;
    let member = resolved.user.id;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let fail = |what: &str, err: sqlx::Error| {
        tracing::warn!("economy: reading {what} of {member} in {guild} failed: {err}");
        BuyError::Db
    };
    let protection = crate::economy::protection(&data.economy_data(), guild, member, now)
        .await
        .map_err(|e| fail("protection", e))?;
    let mirrors = economy::active_of_kind(pool, guild, member, &[ActionKind::Mirror])
        .await
        .map_err(|e| fail("mirror", e))?;
    let same_kinds: &[ActionKind] = match kind {
        ActionKind::Rename | ActionKind::Clown => &[ActionKind::Rename, ActionKind::Clown],
        _ => std::slice::from_ref(&kind),
    };
    let same = economy::active_of_kind(pool, guild, member, same_kinds)
        .await
        .map_err(|e| fail("running pranks", e))?;
    let own = {
        let g = cache.guild(guild);
        member_facts(guild, resolved, |r| {
            g.as_ref().and_then(|g| cached_position(g, r))
        })
    };
    let bot_id = cache.current_user().id;
    let bot_top_role_position = top_position(cache, guild, bot_id);
    let (server_mute, server_deaf) = voice_flags(cache, guild, member);
    Ok(TargetFacts {
        is_bot: resolved.user.bot(),
        is_self,
        is_owner: is_owner(cache, guild, member),
        top_role_position: own.top_role_position,
        bot_top_role_position,
        in_voice: voice_of(cache, guild, member),
        server_mute,
        server_deaf,
        opted_out: data.economy.is_opted_out(guild, member),
        protection,
        has_mirror: mirrors.iter().any(|r| r.state == ActionState::Active),
        running_same_kind: running_end(&same),
        can_join_destination: if kind == ActionKind::Drag {
            buyer_voice.and_then(|ch| can_join(cache, guild, member, ch))
        } else {
            None
        },
        current_nick: own.current_nick,
        display_name: own.display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::immunity::Protection;
    use crate::economy::shop::Refusal;
    use crate::economy::target::{check_target, TargetOutcome};
    use crate::rooms::state::OverwriteTarget;

    const VC: ChannelId = ChannelId::new(5);

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    #[test]
    fn overwrites_convert_the_way_rooms_reads_a_category() {
        // The glue feeds StoredOverwrite::from into member_can_join: a deny of
        // Connect on a role the member holds keeps them out.
        let guild = GuildId::new(100);
        let role = RoleId::new(7);
        let ovs = [StoredOverwrite {
            target: OverwriteTarget::Role { id: role },
            allow: Permissions::empty(),
            deny: Permissions::CONNECT,
        }];
        let role_perms: HashMap<RoleId, Permissions> = [(role, Permissions::empty())].into();
        let view = BaselineView {
            guild,
            everyone: Permissions::VIEW_CHANNEL | Permissions::CONNECT,
            role_perms: &role_perms,
            overwrites: &ovs,
        };
        assert!(!member_can_join(&view, UserId::new(1), &[role]));
        assert!(member_can_join(&view, UserId::new(1), &[]));
    }

    mod gather {
        use super::*;
        use crate::economy::settings::EconomySettings;
        use crate::economy::wallet::ActionPayload;
        use crate::DataInner;
        use sqlx::PgPool;
        use std::sync::Arc;

        const G: GuildId = GuildId::new(1);
        const M: UserId = UserId::new(20);
        const BUYER: UserId = UserId::new(10);

        static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

        async fn data(pool: &PgPool) -> Data {
            let s = EconomySettings {
                enabled: true,
                ..EconomySettings::off(G)
            };
            crate::db::economy::save_settings(pool, &s).await.unwrap();
            let data = Data(Arc::new(DataInner {
                database_pool: Some(pool.clone()),
                ..Default::default()
            }));
            assert!(crate::economy::load_settings(&data.economy_data(), G).await);
            data
        }

        /// A running prank of `kind` on `M`, ending an hour after `t0`.
        async fn running(pool: &PgPool, kind: ActionKind, payload: ActionPayload) -> i64 {
            crate::db::economy::credit_earnings(
                pool,
                G,
                &[crate::db::economy::EarnRow {
                    user: BUYER,
                    voice: 100,
                    messages: 0,
                }],
            )
            .await
            .unwrap();
            let until = Some(t0() + chrono::Duration::hours(1));
            let c = crate::db::economy::charge_for_action(
                pool,
                &crate::db::economy::NewAction {
                    guild: G,
                    kind,
                    buyer: BUYER,
                    target: Some(M),
                    price: 10,
                    note: None,
                    signed: false,
                    payload: &payload,
                    expires_at: until,
                },
            )
            .await
            .unwrap();
            crate::db::economy::activate_action(pool, c.action_id, kind, until)
                .await
                .unwrap();
            c.action_id
        }

        async fn facts(data: &Data, kind: ActionKind, who: UserId) -> TargetFacts {
            let mut m = Member::default();
            m.user.id = who;
            m.nick = Some(serenity::small_fixed_array::FixedString::from_str_trunc(
                "Al",
            ));
            gather_facts(
                data,
                &Cache::default(),
                G,
                Subject {
                    member: &m,
                    is_self: false,
                },
                kind,
                Some(VC),
                t0(),
            )
            .await
            .unwrap()
        }

        #[crack_test_macros::db_test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_cache_that_knows_nothing_fails_closed(pool: PgPool) {
            let data = data(&pool).await;
            let f = facts(&data, ActionKind::Drag, M).await;
            assert_eq!(f.top_role_position, None, "no roles to place it by");
            assert_eq!(f.bot_top_role_position, None);
            assert_eq!(f.can_join_destination, None, "a drag is refused");
            assert_eq!(f.in_voice, None);
            assert_eq!((f.server_mute, f.server_deaf), (false, false));
            assert!(!f.is_owner);
            // The names are the passed-in member's own.
            assert_eq!(f.current_nick, Some("Al".into()));
            assert_eq!(f.display_name, "Al");
            // And that is what check_target makes of it.
            assert_eq!(
                check_target(ActionKind::Rename, &f, None),
                Err(Refusal::CantTell)
            );
        }

        #[crack_test_macros::db_test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_second_theme_is_refused_with_the_first_ones_end(pool: PgPool) {
            let data = data(&pool).await;
            let theme = || ActionPayload::Theme {
                url: "https://youtu.be/dQw4w9WgXcQ".into(),
                start_secs: 0,
            };
            // Nothing waiting yet: a theme lands.
            let f = facts(&data, ActionKind::Theme, M).await;
            assert_eq!(f.running_same_kind, None);
            assert_eq!(
                check_target(ActionKind::Theme, &f, None),
                Ok(TargetOutcome::Lands)
            );
            // A theme is escrow: bought straight into `active`, as the shop does.
            crate::db::economy::credit_earnings(
                &pool,
                G,
                &[crate::db::economy::EarnRow {
                    user: BUYER,
                    voice: 100,
                    messages: 0,
                }],
            )
            .await
            .unwrap();
            crate::db::economy::charge_active(
                &pool,
                &crate::db::economy::NewAction {
                    guild: G,
                    kind: ActionKind::Theme,
                    buyer: BUYER,
                    target: Some(M),
                    price: 10,
                    note: None,
                    signed: false,
                    payload: &theme(),
                    expires_at: Some(t0() + chrono::Duration::hours(1)),
                },
                crate::economy::wallet::LedgerKind::Purchase,
            )
            .await
            .unwrap();
            let end = Some(t0() + chrono::Duration::hours(1));
            let f = facts(&data, ActionKind::Theme, M).await;
            assert_eq!(f.running_same_kind, end);
            assert_eq!(
                check_target(ActionKind::Theme, &f, None),
                Err(Refusal::AlreadyRunning {
                    until: t0() + chrono::Duration::hours(1)
                })
            );
            // Another kind of purchase on the same member is not blocked by it.
            assert_eq!(
                facts(&data, ActionKind::Bounty, M).await.running_same_kind,
                None
            );
        }

        #[crack_test_macros::db_test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn name_pranks_share_a_slot_and_a_lock_has_its_own(pool: PgPool) {
            let data = data(&pool).await;
            let rename = ActionPayload::Rename {
                name: "x".into(),
                restore: None,
            };
            running(&pool, ActionKind::Rename, rename).await;
            let end = Some(t0() + chrono::Duration::hours(1));
            assert_eq!(
                facts(&data, ActionKind::Rename, M).await.running_same_kind,
                end
            );
            assert_eq!(
                facts(&data, ActionKind::Clown, M).await.running_same_kind,
                end
            );
            assert_eq!(
                facts(&data, ActionKind::Lock, M).await.running_same_kind,
                None
            );
            assert_eq!(
                facts(&data, ActionKind::Mute, M).await.running_same_kind,
                None
            );
            // Someone else has nothing running.
            let other = facts(&data, ActionKind::Rename, UserId::new(21)).await;
            assert_eq!(other.running_same_kind, None);
            // A winding-down prank no longer blocks the slot.
            sqlx::query("UPDATE economy_actions SET state = 'ending'")
                .execute(&pool)
                .await
                .unwrap();
            assert_eq!(
                facts(&data, ActionKind::Rename, M).await.running_same_kind,
                None
            );
        }

        #[crack_test_macros::db_test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn mirror_protection_and_opt_out_are_read(pool: PgPool) {
            let data = data(&pool).await;
            assert!(!facts(&data, ActionKind::Mute, M).await.has_mirror);
            running(&pool, ActionKind::Mirror, ActionPayload::Mirror).await;
            assert!(facts(&data, ActionKind::Mute, M).await.has_mirror);
            assert!(
                !facts(&data, ActionKind::Mute, UserId::new(21))
                    .await
                    .has_mirror
            );
            // A spent mirror is gone.
            assert!(crate::db::economy::consume_mirror(&pool, G, M)
                .await
                .unwrap());
            assert!(!facts(&data, ActionKind::Mute, M).await.has_mirror);

            assert_eq!(facts(&data, ActionKind::Mute, M).await.protection, None);
            crate::db::economy::extend_bought(&pool, G, M, 2, t0())
                .await
                .unwrap();
            assert_eq!(
                facts(&data, ActionKind::Mute, M).await.protection,
                Some(Protection::Until(t0() + chrono::Duration::hours(2)))
            );
            data.economy.set_one_opted_out(G, M, true);
            assert!(facts(&data, ActionKind::Mute, M).await.opted_out);
        }
    }
}
