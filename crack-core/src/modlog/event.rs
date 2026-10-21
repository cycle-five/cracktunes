//! The events the mod log posts, and the one embed each renders to.
use crate::economy::settings::ActionKind;
use crate::messaging::format::{clip, escape, DESCRIPTION_MAX, EMBED_TITLE_MAX, FIELD_MAX};
use crate::messaging::messages::*;
use crate::messaging::render::{Mentions, RenderCx, Rendered};
use chrono::{DateTime, Utc};
use serenity::all::{ChannelId, CreateEmbed, CreateEmbedFooter, Timestamp, UserId};

/// How many members a batch names before "and N more".
const BATCH_NAMES_MAX: usize = 10;

/// Who took an action from Discord's audit log, and whether that account is
/// a bot. Another bot's mutes, disconnects and moves are logged like a
/// moderator's and marked as a bot's (owner's call, 2026-10-18): the action
/// still happened, and the mark says where to look next. There is no
/// `From<UserId>`: every producer states the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Actor {
    pub id: UserId,
    pub bot: bool,
}

impl Actor {
    pub const fn person(id: UserId) -> Self {
        Self { id, bot: false }
    }

    pub const fn bot(id: UserId) -> Self {
        Self { id, bot: true }
    }
}

/// Who a forced disconnect or move is credited to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    By(Actor),
    OneOf(Vec<Actor>),
}

/// One thing the mod log reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModLogEvent {
    ServerMute {
        target: UserId,
        actor: Actor,
        on: bool,
        reason: Option<String>,
    },
    ServerDeafen {
        target: UserId,
        actor: Actor,
        on: bool,
        reason: Option<String>,
    },
    Disconnect {
        target: UserId,
        from: ChannelId,
        by: Attribution,
    },
    Move {
        target: UserId,
        from: ChannelId,
        to: ChannelId,
        by: Attribution,
    },
    DisconnectBatch {
        actors: Vec<Actor>,
        count: u32,
        among: Vec<UserId>,
    },
    RoomKick {
        target: UserId,
        room: ChannelId,
        actor: UserId,
        reason: String,
    },
    RoomUnblock {
        target: UserId,
        room: ChannelId,
        actor: UserId,
    },
    RoomMute {
        target: UserId,
        room: ChannelId,
        actor: UserId,
        on: bool,
        reason: Option<String>,
    },
    ImmunityGranted {
        by: UserId,
        target: UserId,
        until: Option<DateTime<Utc>>,
        reason: Option<String>,
    },
    ImmunityLifted {
        by: UserId,
        target: UserId,
    },
    CreditsAdjusted {
        by: UserId,
        target: UserId,
        amount: i64,
        reason: String,
    },
    /// A shop purchase that changed a member. The buyer is always named
    /// here, signed or not (owner ruling): moderators see who did it.
    ShopAction {
        action: ActionKind,
        buyer: UserId,
        target: UserId,
        note: Option<String>,
        signed: bool,
        until: Option<DateTime<Utc>>,
        /// Whose mirror bounced the prank onto the buyer.
        mirrored_from: Option<UserId>,
    },
    /// The pranks running on `target` were ended early: by `target`'s own
    /// cleanse (`by == target`) or a moderator's `/economy cancel`.
    ShopCancelled {
        by: UserId,
        target: UserId,
    },
}

/// For logs: the kind of event.
impl std::fmt::Display for ModLogEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            Self::ServerMute { .. } => "server_mute",
            Self::ServerDeafen { .. } => "server_deafen",
            Self::Disconnect { .. } => "disconnect",
            Self::Move { .. } => "move",
            Self::DisconnectBatch { .. } => "disconnect_batch",
            Self::RoomKick { .. } => "room_kick",
            Self::RoomUnblock { .. } => "room_unblock",
            Self::RoomMute { .. } => "room_mute",
            Self::ImmunityGranted { .. } => "immunity_granted",
            Self::ImmunityLifted { .. } => "immunity_lifted",
            Self::CreditsAdjusted { .. } => "credits_adjusted",
            Self::ShopAction { .. } => "shop_action",
            Self::ShopCancelled { .. } => "shop_cancelled",
        };
        write!(f, "ModLog({kind})")
    }
}

impl From<ModLogEvent> for crate::messaging::message::CrackedMessage {
    fn from(e: ModLogEvent) -> Self {
        Self::ModLog(Box::new(e))
    }
}

fn user(u: UserId) -> String {
    format!("<@{u}>")
}

fn channel(c: ChannelId) -> String {
    format!("<#{c}>")
}

fn users(list: &[UserId]) -> String {
    list.iter().map(|u| user(*u)).collect::<Vec<_>>().join(", ")
}

/// An audit actor's mention, marked when the account is a bot.
fn who(a: Actor) -> String {
    if a.bot {
        format!("{} {MODLOG_BOT_MARK}", user(a.id))
    } else {
        user(a.id)
    }
}

fn whos(list: &[Actor]) -> String {
    list.iter().map(|a| who(*a)).collect::<Vec<_>>().join(", ")
}

fn by(a: &Attribution) -> String {
    match a {
        Attribution::By(a) => format!("{MODLOG_BY} {}", who(*a)),
        Attribution::OneOf(l) => format!("{MODLOG_BY_ONE_OF} {}", whos(l)),
    }
}

/// Everything that varies per event.
struct Parts {
    title: &'static str,
    description: String,
    reason: Option<String>,
    audit: bool,
}

fn toggle(on: bool, yes: &'static str, no: &'static str) -> &'static str {
    if on {
        yes
    } else {
        no
    }
}

fn parts(e: &ModLogEvent) -> Parts {
    let plain = |title, description| Parts {
        title,
        description,
        reason: None,
        audit: false,
    };
    match e {
        ModLogEvent::ServerMute {
            target,
            actor,
            on,
            reason,
        } => Parts {
            title: toggle(*on, MODLOG_TITLE_MUTE, MODLOG_TITLE_UNMUTE),
            description: format!(
                "{} {} {}",
                who(*actor),
                toggle(*on, MODLOG_MUTED, MODLOG_UNMUTED),
                user(*target),
            ),
            reason: reason.clone(),
            audit: false,
        },
        ModLogEvent::ServerDeafen {
            target,
            actor,
            on,
            reason,
        } => Parts {
            title: toggle(*on, MODLOG_TITLE_DEAFEN, MODLOG_TITLE_UNDEAFEN),
            description: format!(
                "{} {} {}",
                who(*actor),
                toggle(*on, MODLOG_DEAFENED, MODLOG_UNDEAFENED),
                user(*target),
            ),
            reason: reason.clone(),
            audit: false,
        },
        ModLogEvent::Disconnect {
            target,
            from,
            by: a,
        } => Parts {
            audit: true,
            ..plain(
                MODLOG_TITLE_DISCONNECT,
                format!(
                    "{} {MODLOG_FROM} {} {}",
                    user(*target),
                    channel(*from),
                    by(a)
                ),
            )
        },
        ModLogEvent::Move {
            target,
            from,
            to,
            by: a,
        } => Parts {
            audit: true,
            ..plain(
                MODLOG_TITLE_MOVE,
                format!(
                    "{} {MODLOG_FROM} {} {MODLOG_TO} {} {}",
                    user(*target),
                    channel(*from),
                    channel(*to),
                    by(a)
                ),
            )
        },
        ModLogEvent::DisconnectBatch {
            actors,
            count,
            among,
        } => {
            let shown = users(&among[..among.len().min(BATCH_NAMES_MAX)]);
            let rest = among.len().saturating_sub(BATCH_NAMES_MAX);
            let more = if rest > 0 {
                format!(" {MODLOG_AND} {rest} {MODLOG_MORE}")
            } else {
                String::new()
            };
            // Several actors: which of them disconnected whom is unknown.
            let credited = match actors.as_slice() {
                [_, _, ..] => format!("{MODLOG_ONE_OR_MORE_OF} {}", whos(actors)),
                _ => whos(actors),
            };
            Parts {
                audit: true,
                ..plain(
                    MODLOG_TITLE_BATCH,
                    format!("{credited} {MODLOG_DISCONNECTED} {count} {MODLOG_OF}: {shown}{more}"),
                )
            }
        },
        ModLogEvent::RoomKick {
            target,
            room,
            actor,
            reason,
        } => Parts {
            reason: Some(reason.clone()),
            ..plain(
                MODLOG_TITLE_ROOM_KICK,
                format!(
                    "{} {MODLOG_KICKED} {} {MODLOG_FROM} {}",
                    user(*actor),
                    user(*target),
                    channel(*room)
                ),
            )
        },
        ModLogEvent::RoomUnblock {
            target,
            room,
            actor,
        } => plain(
            MODLOG_TITLE_ROOM_UNBLOCK,
            format!(
                "{} {MODLOG_LET_BACK_IN} {} {MODLOG_IN} {}",
                user(*actor),
                user(*target),
                channel(*room)
            ),
        ),
        ModLogEvent::RoomMute {
            target,
            room,
            actor,
            on,
            reason,
        } => Parts {
            reason: reason.clone(),
            ..plain(
                toggle(*on, MODLOG_TITLE_ROOM_MUTE, MODLOG_TITLE_ROOM_UNMUTE),
                format!(
                    "{} {} {} {MODLOG_IN} {}",
                    user(*actor),
                    toggle(*on, MODLOG_MUTED, MODLOG_UNMUTED),
                    user(*target),
                    channel(*room)
                ),
            )
        },
        ModLogEvent::ImmunityGranted {
            by,
            target,
            until,
            reason,
        } => {
            let until = until.map_or_else(
                || MODLOG_UNTIL_LIFTED.to_owned(),
                |t| format!("{MODLOG_UNTIL} <t:{}:f>", t.timestamp()),
            );
            Parts {
                reason: reason.clone(),
                ..plain(
                    MODLOG_TITLE_IMMUNITY_GRANTED,
                    format!(
                        "{} {MODLOG_GRANTED_IMMUNITY_TO} {} {until}",
                        user(*by),
                        user(*target)
                    ),
                )
            }
        },
        ModLogEvent::ImmunityLifted { by, target } => plain(
            MODLOG_TITLE_IMMUNITY_LIFTED,
            format!(
                "{} {MODLOG_LIFTED_IMMUNITY_OF} {}",
                user(*by),
                user(*target)
            ),
        ),
        ModLogEvent::CreditsAdjusted {
            by,
            target,
            amount,
            reason,
        } => {
            let signed = if *amount < 0 {
                format!("{MODLOG_MINUS}{}", amount.unsigned_abs())
            } else {
                format!("+{amount}")
            };
            Parts {
                reason: Some(reason.clone()),
                ..plain(
                    MODLOG_TITLE_CREDITS_ADJUSTED,
                    format!(
                        "{} {MODLOG_ADJUSTED_CREDITS_OF} {} {MODLOG_BY} {signed}",
                        user(*by),
                        user(*target)
                    ),
                )
            }
        },
        ModLogEvent::ShopAction {
            action,
            buyer,
            target,
            note,
            signed,
            until,
            mirrored_from,
        } => {
            let mut d = format!(
                "{} {MODLOG_SHOP_USED} {} {MODLOG_SHOP_ON} {}",
                user(*buyer),
                action.label(),
                user(*target)
            );
            if let Some(t) = until {
                d.push_str(&format!(" {MODLOG_UNTIL} <t:{}:f>", t.timestamp()));
            }
            if let Some(owner) = mirrored_from {
                d.push_str(&format!(
                    " {MODLOG_SHOP_BOUNCED_PREFIX} {}{MODLOG_SHOP_BOUNCED_SUFFIX}",
                    user(*owner)
                ));
            }
            d.push_str(&format!(
                " ({})",
                toggle(*signed, MODLOG_SHOP_SIGNED, MODLOG_SHOP_ANONYMOUS)
            ));
            Parts {
                reason: note.clone(),
                ..plain(MODLOG_TITLE_SHOP_ACTION, d)
            }
        },
        ModLogEvent::ShopCancelled { by, target } => plain(
            MODLOG_TITLE_SHOP_CANCELLED,
            format!("{} {MODLOG_SHOP_CLEANSED} {}", user(*by), user(*target)),
        ),
    }
}

/// The one embed an event renders to. Mentions sit inside the embed and
/// nobody is pinged.
#[must_use]
pub fn render(e: &ModLogEvent, cx: &RenderCx) -> Rendered {
    let p = parts(e);
    let mut embed = CreateEmbed::new()
        .title(clip(p.title, EMBED_TITLE_MAX))
        .description(clip(&p.description, DESCRIPTION_MAX));
    if let Some(reason) = p.reason.as_deref().filter(|r| !r.trim().is_empty()) {
        embed = embed.field(MODLOG_REASON_FIELD, clip(&escape(reason), FIELD_MAX), false);
    }
    if p.audit {
        embed = embed.footer(CreateEmbedFooter::new(MODLOG_AUDIT_FOOTER));
    }
    if let Ok(ts) = Timestamp::from_unix_timestamp(cx.now_unix) {
        embed = embed.timestamp(ts);
    }
    Rendered {
        mentions: Mentions::None,
        ..Rendered::embed(embed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: UserId = UserId::new(11);
    const A: UserId = UserId::new(22);
    const R: ChannelId = ChannelId::new(333);
    const R2: ChannelId = ChannelId::new(444);

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 1_700_000_000,
            embed_links: true,
        }
    }

    fn json(r: &Rendered) -> serde_json::Value {
        serde_json::to_value(r.embed.as_ref().unwrap()).unwrap()
    }

    fn ids(n: u64) -> Vec<UserId> {
        (1..=n).map(|i| UserId::new(1000 + i)).collect()
    }

    fn all() -> Vec<ModLogEvent> {
        vec![
            ModLogEvent::ServerMute {
                target: T,
                actor: Actor::person(A),
                on: true,
                reason: Some("loud".into()),
            },
            ModLogEvent::ServerMute {
                target: T,
                actor: Actor::person(A),
                on: false,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: T,
                actor: Actor::person(A),
                on: true,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: T,
                actor: Actor::person(A),
                on: false,
                reason: None,
            },
            ModLogEvent::Disconnect {
                target: T,
                from: R,
                by: Attribution::By(Actor::person(A)),
            },
            ModLogEvent::Move {
                target: T,
                from: R,
                to: R2,
                by: Attribution::OneOf(vec![Actor::person(A), Actor::person(T)]),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![Actor::person(A)],
                count: 2,
                among: ids(3),
            },
            ModLogEvent::RoomKick {
                target: T,
                room: R,
                actor: A,
                reason: "spam".into(),
            },
            ModLogEvent::RoomUnblock {
                target: T,
                room: R,
                actor: A,
            },
            ModLogEvent::RoomMute {
                target: T,
                room: R,
                actor: A,
                on: true,
                reason: None,
            },
            ModLogEvent::RoomMute {
                target: T,
                room: R,
                actor: A,
                on: false,
                reason: None,
            },
            ModLogEvent::ImmunityGranted {
                by: A,
                target: T,
                until: None,
                reason: None,
            },
            ModLogEvent::ImmunityLifted { by: A, target: T },
            ModLogEvent::CreditsAdjusted {
                by: A,
                target: T,
                amount: 5,
                reason: "x".into(),
            },
            ModLogEvent::ShopAction {
                action: ActionKind::Rename,
                buyer: A,
                target: T,
                note: Some("hi".into()),
                signed: false,
                until: None,
                mirrored_from: None,
            },
            ModLogEvent::ShopCancelled { by: A, target: T },
        ]
    }

    #[test]
    fn every_event_renders_one_embed_and_pings_nobody() {
        let mut titles = std::collections::HashSet::new();
        for e in all() {
            let r = render(&e, &cx());
            assert_eq!(r.mentions, Mentions::None, "{e:?}");
            assert!(r.embeds_extra.is_empty() && r.content.is_none(), "{e:?}");
            let j = json(&r);
            let title = j["title"].as_str().unwrap().to_owned();
            assert!(!title.is_empty());
            assert!(titles.insert(title), "duplicate title for {e:?}");
            assert!(!j["description"].as_str().unwrap().is_empty());
            assert_eq!(
                j["timestamp"].as_str().unwrap()[..19],
                *"2023-11-14T22:13:20",
                "{e:?}"
            );
        }
    }

    /// One event for a member's cleanse and a moderator's cancel, so its
    /// words fit both.
    #[test]
    fn ended_pranks_read_the_same_for_a_cleanse_and_a_cancel() {
        for by in [A, T] {
            let j = json(&render(
                &ModLogEvent::ShopCancelled { by, target: T },
                &cx(),
            ));
            assert_eq!(j["title"], "🧼 Shop pranks ended");
            assert_eq!(
                j["description"],
                format!("<@{by}> ended the pranks on <@{T}>").as_str()
            );
        }
    }

    /// Another bot's action is logged like a moderator's, with a mark after
    /// its mention; a person's carries none.
    #[test]
    fn a_bot_actor_is_marked_as_a_bot() {
        let bot = Actor::bot(A);
        let marked = format!("<@22> {MODLOG_BOT_MARK}");
        for e in [
            ModLogEvent::ServerMute {
                target: T,
                actor: bot,
                on: true,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: T,
                actor: bot,
                on: false,
                reason: None,
            },
            ModLogEvent::Disconnect {
                target: T,
                from: R,
                by: Attribution::By(bot),
            },
            ModLogEvent::Move {
                target: T,
                from: R,
                to: R2,
                by: Attribution::OneOf(vec![Actor::person(UserId::new(5)), bot]),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![bot],
                count: 1,
                among: vec![T],
            },
        ] {
            let j = json(&render(&e, &cx()));
            let d = j["description"].as_str().unwrap();
            assert!(d.contains(&marked), "{e}: {d}");
        }
        let person = ModLogEvent::ServerMute {
            target: T,
            actor: Actor::person(A),
            on: true,
            reason: None,
        };
        let j = json(&render(&person, &cx()));
        assert_eq!(j["description"], format!("<@22> {MODLOG_MUTED} <@11>"));
    }

    #[test]
    fn shop_action_renders_buyer_even_unsigned() {
        let until = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let e = ModLogEvent::ShopAction {
            action: ActionKind::Rename,
            buyer: A,
            target: T,
            note: Some("gg".into()),
            signed: false,
            until: Some(until),
            mirrored_from: None,
        };
        let j = json(&render(&e, &cx()));
        assert_eq!(
            j["description"],
            "<@22> used Rename on <@11> until <t:1800000000:f> (anonymous)"
        );
        assert_eq!(j["fields"][0]["value"], "gg");
        let bounced = ModLogEvent::ShopAction {
            action: ActionKind::Rename,
            buyer: A,
            target: T,
            note: None,
            signed: true,
            until: None,
            mirrored_from: Some(T),
        };
        let j = json(&render(&bounced, &cx()));
        assert_eq!(
            j["description"],
            "<@22> used Rename on <@11> (bounced by <@11>'s mirror) (signed)"
        );
    }

    #[test]
    fn room_kick_shows_actor_target_room_and_reason() {
        let r = render(
            &ModLogEvent::RoomKick {
                target: T,
                room: R,
                actor: A,
                reason: "spam".into(),
            },
            &cx(),
        );
        let j = json(&r);
        assert_eq!(j["title"], MODLOG_TITLE_ROOM_KICK);
        let d = j["description"].as_str().unwrap();
        assert_eq!(d, "<@22> kicked <@11> from <#333>");
        assert_eq!(j["fields"][0]["name"], "Reason");
        assert_eq!(j["fields"][0]["value"], "spam");
    }

    #[test]
    fn one_of_lists_every_candidate() {
        let r = render(
            &ModLogEvent::Disconnect {
                target: T,
                from: R,
                by: Attribution::OneOf(vec![
                    Actor::person(UserId::new(5)),
                    Actor::person(UserId::new(6)),
                    Actor::person(UserId::new(7)),
                ]),
            },
            &cx(),
        );
        let d = json(&r)["description"].as_str().unwrap().to_owned();
        assert!(d.contains("by one of <@5>, <@6>, <@7>"), "{d}");
        assert!(d.contains("<@11>") && d.contains("<#333>"), "{d}");
    }

    #[test]
    fn batch_caps_the_member_list_at_ten() {
        let d = |n| {
            let r = render(
                &ModLogEvent::DisconnectBatch {
                    actors: vec![Actor::person(A)],
                    count: 2,
                    among: ids(n),
                },
                &cx(),
            );
            json(&r)["description"].as_str().unwrap().to_owned()
        };
        let small = d(3);
        assert_eq!(small, "<@22> disconnected 2 of: <@1001>, <@1002>, <@1003>");
        let ten = d(10);
        assert!(ten.contains("<@1010>") && !ten.contains("more"), "{ten}");
        let big = d(13);
        assert!(big.contains("<@1010>") && !big.contains("<@1011>"), "{big}");
        assert!(big.ends_with("and 3 more"), "{big}");
    }

    #[test]
    fn a_batch_by_several_actors_is_hedged() {
        let r = render(
            &ModLogEvent::DisconnectBatch {
                actors: vec![Actor::person(UserId::new(5)), Actor::person(UserId::new(6))],
                count: 2,
                among: ids(3),
            },
            &cx(),
        );
        let d = json(&r)["description"].as_str().unwrap().to_owned();
        assert_eq!(
            d,
            "One or more of <@5>, <@6> disconnected 2 of: <@1001>, <@1002>, <@1003>"
        );
    }

    #[test]
    fn audit_attributed_events_carry_the_footer_and_exact_ones_do_not() {
        for e in all() {
            let j = json(&render(&e, &cx()));
            let audit = matches!(
                e,
                ModLogEvent::Disconnect { .. }
                    | ModLogEvent::Move { .. }
                    | ModLogEvent::DisconnectBatch { .. }
            );
            if audit {
                assert_eq!(j["footer"]["text"], MODLOG_AUDIT_FOOTER, "{e:?}");
            } else {
                assert!(j.get("footer").is_none(), "{e:?}");
            }
        }
    }

    #[test]
    fn reason_is_escaped_and_capped() {
        let r = render(
            &ModLogEvent::RoomKick {
                target: T,
                room: R,
                actor: A,
                reason: "@everyone **hi**".into(),
            },
            &cx(),
        );
        let v = json(&r)["fields"][0]["value"].as_str().unwrap().to_owned();
        assert_eq!(v, "\\@everyone \\*\\*hi\\*\\*");
        let long = render(
            &ModLogEvent::ServerMute {
                target: T,
                actor: Actor::person(A),
                on: true,
                reason: Some("x".repeat(2000)),
            },
            &cx(),
        );
        let v = json(&long)["fields"][0]["value"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(v.chars().count(), FIELD_MAX);
        assert!(v.ends_with('…'));
    }

    fn fields(r: &Rendered) -> (String, String, Option<String>) {
        let j = json(r);
        (
            j["title"].as_str().unwrap().to_owned(),
            j["description"].as_str().unwrap().to_owned(),
            j["fields"][0]["value"].as_str().map(str::to_owned),
        )
    }

    #[test]
    fn immunity_granted_shows_mod_target_until_and_reason() {
        let until = chrono::DateTime::from_timestamp(1_700_100_000, 0).unwrap();
        let finite = render(
            &ModLogEvent::ImmunityGranted {
                by: A,
                target: T,
                until: Some(until),
                reason: Some("raid cleanup".into()),
            },
            &cx(),
        );
        assert_eq!(
            fields(&finite),
            (
                "🛡️ Immunity granted".to_owned(),
                "<@22> granted immunity to <@11> until <t:1700100000:f>".to_owned(),
                Some("raid cleanup".to_owned()),
            )
        );
        let open = render(
            &ModLogEvent::ImmunityGranted {
                by: A,
                target: T,
                until: None,
                reason: None,
            },
            &cx(),
        );
        assert_eq!(
            fields(&open),
            (
                "🛡️ Immunity granted".to_owned(),
                "<@22> granted immunity to <@11> until lifted".to_owned(),
                None,
            )
        );
    }

    #[test]
    fn immunity_lifted_shows_mod_and_target() {
        let r = render(&ModLogEvent::ImmunityLifted { by: A, target: T }, &cx());
        assert_eq!(
            fields(&r),
            (
                "🛡️ Immunity lifted".to_owned(),
                "<@22> lifted the immunity of <@11>".to_owned(),
                None,
            )
        );
    }

    #[test]
    fn credits_adjusted_shows_signed_amount_and_reason() {
        let up = render(
            &ModLogEvent::CreditsAdjusted {
                by: A,
                target: T,
                amount: 150,
                reason: "event prize".into(),
            },
            &cx(),
        );
        assert_eq!(
            fields(&up),
            (
                "🪙 Credits adjusted".to_owned(),
                "<@22> adjusted the credits of <@11> by +150".to_owned(),
                Some("event prize".to_owned()),
            )
        );
        let down = render(
            &ModLogEvent::CreditsAdjusted {
                by: A,
                target: T,
                amount: -200,
                reason: "mistake".into(),
            },
            &cx(),
        );
        assert_eq!(
            fields(&down),
            (
                "🪙 Credits adjusted".to_owned(),
                "<@22> adjusted the credits of <@11> by \u{2212}200".to_owned(),
                Some("mistake".to_owned()),
            )
        );
    }

    #[test]
    fn economy_events_log_their_kind() {
        let shown: Vec<String> = all().iter().rev().take(5).map(|e| e.to_string()).collect();
        assert_eq!(
            shown,
            [
                "ModLog(shop_cancelled)",
                "ModLog(shop_action)",
                "ModLog(credits_adjusted)",
                "ModLog(immunity_lifted)",
                "ModLog(immunity_granted)"
            ]
        );
    }
}
