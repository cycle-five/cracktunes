//! The events the mod log posts, and the one embed each renders to.
use chrono::{DateTime, Utc};
use crack_common::format::{clip, escape, DESCRIPTION_MAX, EMBED_TITLE_MAX, FIELD_MAX};
use crack_common::messages::*;
use crack_common::render::{RenderCx, Rendered};
use crack_economy::settings::ActionKind;
use crack_model::{Card, Field, Footer, Mention, RoomRef, UserRef};
use crack_types::discord::ToRef;
use serenity::all::UserId;

/// How many members a batch names before "and N more".
const BATCH_NAMES_MAX: usize = 10;

/// Who took an action from Discord's audit log, and whether that account is
/// a bot. Another bot's mutes, disconnects and moves are logged like a
/// moderator's and marked as a bot's (owner's call, 2026-10-18): the action
/// still happened, and the mark says where to look next. There is no
/// `From<UserId>`: every producer states the flag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Actor {
    pub id: UserRef,
    pub bot: bool,
}

impl Actor {
    pub fn new(id: UserId, bot: bool) -> Self {
        Self {
            id: id.to_ref(),
            bot,
        }
    }

    pub fn person(id: UserId) -> Self {
        Self::new(id, false)
    }

    pub fn bot(id: UserId) -> Self {
        Self::new(id, true)
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
        target: UserRef,
        actor: Actor,
        on: bool,
        reason: Option<String>,
    },
    ServerDeafen {
        target: UserRef,
        actor: Actor,
        on: bool,
        reason: Option<String>,
    },
    Disconnect {
        target: UserRef,
        from: RoomRef,
        by: Attribution,
    },
    Move {
        target: UserRef,
        from: RoomRef,
        to: RoomRef,
        by: Attribution,
    },
    DisconnectBatch {
        actors: Vec<Actor>,
        count: u32,
        among: Vec<UserRef>,
    },
    RoomKick {
        target: UserRef,
        room: RoomRef,
        actor: UserRef,
        reason: String,
    },
    RoomUnblock {
        target: UserRef,
        room: RoomRef,
        actor: UserRef,
    },
    RoomMute {
        target: UserRef,
        room: RoomRef,
        actor: UserRef,
        on: bool,
        reason: Option<String>,
    },
    ImmunityGranted {
        by: UserRef,
        target: UserRef,
        until: Option<DateTime<Utc>>,
        reason: Option<String>,
    },
    ImmunityLifted {
        by: UserRef,
        target: UserRef,
    },
    CreditsAdjusted {
        by: UserRef,
        target: UserRef,
        amount: i64,
        reason: String,
    },
    /// A shop purchase aimed at a member. The buyer is always named
    /// here, signed or not (owner ruling): moderators see who did it.
    ShopAction {
        action: ActionKind,
        buyer: UserRef,
        target: UserRef,
        note: Option<String>,
        signed: bool,
        until: Option<DateTime<Utc>>,
        /// Whose mirror bounced the prank onto the buyer.
        mirrored_from: Option<UserRef>,
    },
    /// The pranks running on `target` were ended early: by `target`'s own
    /// cleanse (`by == target`) or a moderator's `/economy cancel`.
    ShopCancelled {
        by: UserRef,
        target: UserRef,
    },
    /// A crowdfund was opened (with the starter's own first pledge). The
    /// starter is always named here, signed or not, as a shop purchase's
    /// buyer is; `signed` says whether the public card named them too.
    CrowdfundStarted {
        action: ActionKind,
        starter: UserRef,
        target: UserRef,
        note: Option<String>,
        signed: bool,
    },
    /// A filled crowdfund landed. Every contributor is named, signed or not:
    /// the starter first, then each pledger once in pledge order.
    CrowdfundExecuted {
        action: ActionKind,
        target: UserRef,
        contributors: Vec<UserRef>,
        until: Option<DateTime<Utc>>,
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
            Self::CrowdfundStarted { .. } => "crowdfund_started",
            Self::CrowdfundExecuted { .. } => "crowdfund_executed",
        };
        write!(f, "ModLog({kind})")
    }
}

impl From<ModLogEvent> for crack_common::message::CrackedMessage {
    fn from(e: ModLogEvent) -> Self {
        Self::ModLog(Box::new(e))
    }
}

fn user(u: &UserRef) -> String {
    Mention::User(u.clone()).to_string()
}

fn channel(c: &RoomRef) -> String {
    Mention::Room(c.clone()).to_string()
}

fn users(list: &[UserRef]) -> String {
    list.iter().map(user).collect::<Vec<_>>().join(", ")
}

/// An audit actor's mention, marked when the account is a bot.
fn who(a: &Actor) -> String {
    if a.bot {
        format!("{} {MODLOG_BOT_MARK}", user(&a.id))
    } else {
        user(&a.id)
    }
}

fn whos(list: &[Actor]) -> String {
    list.iter().map(who).collect::<Vec<_>>().join(", ")
}

fn by(a: &Attribution) -> String {
    match a {
        Attribution::By(a) => format!("{MODLOG_BY} {}", who(a)),
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
                who(actor),
                toggle(*on, MODLOG_MUTED, MODLOG_UNMUTED),
                user(target),
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
                who(actor),
                toggle(*on, MODLOG_DEAFENED, MODLOG_UNDEAFENED),
                user(target),
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
                format!("{} {MODLOG_FROM} {} {}", user(target), channel(from), by(a)),
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
                    user(target),
                    channel(from),
                    channel(to),
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
                    user(actor),
                    user(target),
                    channel(room)
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
                user(actor),
                user(target),
                channel(room)
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
                    user(actor),
                    toggle(*on, MODLOG_MUTED, MODLOG_UNMUTED),
                    user(target),
                    channel(room)
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
                        user(by),
                        user(target)
                    ),
                )
            }
        },
        ModLogEvent::ImmunityLifted { by, target } => plain(
            MODLOG_TITLE_IMMUNITY_LIFTED,
            format!("{} {MODLOG_LIFTED_IMMUNITY_OF} {}", user(by), user(target)),
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
                        user(by),
                        user(target)
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
                user(buyer),
                action.label(),
                user(target)
            );
            if let Some(t) = until {
                d.push_str(&format!(" {MODLOG_UNTIL} <t:{}:f>", t.timestamp()));
            }
            if let Some(owner) = mirrored_from {
                d.push_str(&format!(
                    " {MODLOG_SHOP_BOUNCED_PREFIX} {}{MODLOG_SHOP_BOUNCED_SUFFIX}",
                    user(owner)
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
            format!("{} {MODLOG_SHOP_CLEANSED} {}", user(by), user(target)),
        ),
        ModLogEvent::CrowdfundStarted {
            action,
            starter,
            target,
            note,
            signed,
        } => Parts {
            reason: note.clone(),
            ..plain(
                MODLOG_TITLE_CROWDFUND_STARTED,
                format!(
                    "{} {MODLOG_CROWDFUND_STARTED} {} {MODLOG_SHOP_ON} {} ({})",
                    user(starter),
                    action.label(),
                    user(target),
                    toggle(*signed, MODLOG_SHOP_SIGNED, MODLOG_SHOP_ANONYMOUS)
                ),
            )
        },
        ModLogEvent::CrowdfundExecuted {
            action,
            target,
            contributors,
            until,
        } => {
            // The action and the target first, so a long list of
            // contributors can only ever clip its own tail.
            let mut d = format!("{} {MODLOG_SHOP_ON} {}", action.label(), user(target));
            if let Some(t) = until {
                d.push_str(&format!(" {MODLOG_UNTIL} <t:{}:f>", t.timestamp()));
            }
            d.push_str(&format!(
                " ({MODLOG_CROWDFUNDED_BY} {})",
                users(contributors)
            ));
            plain(MODLOG_TITLE_CROWDFUND, d)
        },
    }
}

/// The one embed an event renders to. Mentions sit inside the embed and
/// nobody is pinged.
#[must_use]
pub fn render(e: &ModLogEvent, cx: &RenderCx) -> Rendered {
    let p = parts(e);
    let mut card = Card {
        title: Some(clip(p.title, EMBED_TITLE_MAX)),
        description: Some(clip(&p.description, DESCRIPTION_MAX)),
        ..Card::default()
    };
    if let Some(reason) = p.reason.as_deref().filter(|r| !r.trim().is_empty()) {
        card.fields.push(Field {
            name: MODLOG_REASON_FIELD.to_string(),
            value: clip(&escape(reason), FIELD_MAX),
            inline: false,
        });
    }
    if p.audit {
        card.footer = Some(Footer {
            text: MODLOG_AUDIT_FOOTER.to_string(),
            icon_url: None,
        });
    }
    card.timestamp = Some(cx.now_unix);
    // `Mentions::default()` pings nobody; the tests pin it.
    Rendered::card(card)
}

impl crack_common::render::Payload for ModLogEvent {
    fn render(&self, cx: &RenderCx) -> Rendered {
        render(self, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_common::render::Mentions;

    const T: UserId = UserId::new(11);
    const A: UserId = UserId::new(22);
    const R: serenity::all::ChannelId = serenity::all::ChannelId::new(333);
    const R2: serenity::all::ChannelId = serenity::all::ChannelId::new(444);
    const R_USER: UserId = UserId::new(555);

    fn t() -> UserRef {
        T.to_ref()
    }

    fn a() -> UserRef {
        A.to_ref()
    }

    fn r() -> RoomRef {
        R.to_ref()
    }

    fn r2() -> RoomRef {
        R2.to_ref()
    }

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 1_700_000_000,
            embed_links: true,
        }
    }

    fn json(r: &Rendered) -> serde_json::Value {
        serde_json::to_value(crack_common::discord::first_embed(r).unwrap()).unwrap()
    }

    fn ids(n: u64) -> Vec<UserRef> {
        (1..=n).map(|i| UserId::new(1000 + i).to_ref()).collect()
    }

    fn all() -> Vec<ModLogEvent> {
        vec![
            ModLogEvent::ServerMute {
                target: t(),
                actor: Actor::person(A),
                on: true,
                reason: Some("loud".into()),
            },
            ModLogEvent::ServerMute {
                target: t(),
                actor: Actor::person(A),
                on: false,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: t(),
                actor: Actor::person(A),
                on: true,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: t(),
                actor: Actor::person(A),
                on: false,
                reason: None,
            },
            ModLogEvent::Disconnect {
                target: t(),
                from: r(),
                by: Attribution::By(Actor::person(A)),
            },
            ModLogEvent::Move {
                target: t(),
                from: r(),
                to: r2(),
                by: Attribution::OneOf(vec![Actor::person(A), Actor::person(T)]),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![Actor::person(A)],
                count: 2,
                among: ids(3),
            },
            ModLogEvent::RoomKick {
                target: t(),
                room: r(),
                actor: a(),
                reason: "spam".into(),
            },
            ModLogEvent::RoomUnblock {
                target: t(),
                room: r(),
                actor: a(),
            },
            ModLogEvent::RoomMute {
                target: t(),
                room: r(),
                actor: a(),
                on: true,
                reason: None,
            },
            ModLogEvent::RoomMute {
                target: t(),
                room: r(),
                actor: a(),
                on: false,
                reason: None,
            },
            ModLogEvent::ImmunityGranted {
                by: a(),
                target: t(),
                until: None,
                reason: None,
            },
            ModLogEvent::ImmunityLifted {
                by: a(),
                target: t(),
            },
            ModLogEvent::CreditsAdjusted {
                by: a(),
                target: t(),
                amount: 5,
                reason: "x".into(),
            },
            ModLogEvent::ShopAction {
                action: ActionKind::Rename,
                buyer: a(),
                target: t(),
                note: Some("hi".into()),
                signed: false,
                until: None,
                mirrored_from: None,
            },
            ModLogEvent::ShopCancelled {
                by: a(),
                target: t(),
            },
            ModLogEvent::CrowdfundStarted {
                action: ActionKind::Mute,
                starter: a(),
                target: t(),
                note: None,
                signed: false,
            },
            ModLogEvent::CrowdfundExecuted {
                action: ActionKind::Mute,
                target: t(),
                contributors: vec![a(), t()],
                until: None,
            },
        ]
    }

    /// Everything a render sends, as exact bytes.
    fn sent(r: &Rendered) -> String {
        format!(
            "{:?}|{}|{}|{}|{:?}",
            r.content,
            serde_json::to_string(&crack_common::discord::first_embed(r)).unwrap(),
            serde_json::to_string(&crack_common::discord::extra_embeds(r)).unwrap(),
            serde_json::to_string(&crack_common::discord::action_rows(&r.rows)).unwrap(),
            r.mentions
        )
    }

    fn pin_cases() -> Vec<ModLogEvent> {
        let until = DateTime::<Utc>::from_timestamp(1_700_003_600, 0);
        let mut out = all();
        out.extend([
            ModLogEvent::ServerMute {
                target: t(),
                actor: Actor::bot(A),
                on: true,
                reason: Some("  ".into()),
            },
            ModLogEvent::Disconnect {
                target: t(),
                from: r(),
                by: Attribution::By(Actor::bot(A)),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![Actor::person(A), Actor::bot(T)],
                count: 14,
                among: ids(14),
            },
            ModLogEvent::RoomKick {
                target: t(),
                room: r(),
                actor: a(),
                reason: "@everyone *x*".into(),
            },
            ModLogEvent::RoomMute {
                target: t(),
                room: r(),
                actor: a(),
                on: true,
                reason: Some("shh".into()),
            },
            ModLogEvent::ImmunityGranted {
                by: a(),
                target: t(),
                until,
                reason: Some("ok".into()),
            },
            ModLogEvent::CreditsAdjusted {
                by: a(),
                target: t(),
                amount: -5,
                reason: "x".into(),
            },
            ModLogEvent::ShopAction {
                action: ActionKind::Mute,
                buyer: a(),
                target: t(),
                note: None,
                signed: true,
                until,
                mirrored_from: Some(R_USER.to_ref()),
            },
            ModLogEvent::CrowdfundStarted {
                action: ActionKind::Mute,
                starter: a(),
                target: t(),
                note: Some("n".into()),
                signed: true,
            },
            ModLogEvent::CrowdfundExecuted {
                action: ActionKind::Mute,
                target: t(),
                contributors: vec![a(), t()],
                until,
            },
        ]);
        out
    }

    #[test]
    fn modlog_events_render_as_before() {
        let cases = pin_cases();
        assert_eq!(cases.len(), EXPECTED.len());
        for (e, expected) in cases.iter().zip(EXPECTED) {
            let actual = sent(&render(e, &cx()));
            assert_eq!(&actual, expected, "{e:?}: {actual}");
        }
    }

    #[test]
    fn every_event_renders_one_embed_and_pings_nobody() {
        let mut titles = std::collections::HashSet::new();
        for e in all() {
            let r = render(&e, &cx());
            assert_eq!(r.mentions, Mentions::None, "{e:?}");
            assert!(r.cards.len() <= 1 && r.content.is_none(), "{e:?}");
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
                &ModLogEvent::ShopCancelled {
                    by: by.to_ref(),
                    target: t(),
                },
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
                target: t(),
                actor: bot.clone(),
                on: true,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: t(),
                actor: bot.clone(),
                on: false,
                reason: None,
            },
            ModLogEvent::Disconnect {
                target: t(),
                from: r(),
                by: Attribution::By(bot.clone()),
            },
            ModLogEvent::Move {
                target: t(),
                from: r(),
                to: r2(),
                by: Attribution::OneOf(vec![Actor::person(UserId::new(5)), bot.clone()]),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![bot.clone()],
                count: 1,
                among: vec![t()],
            },
        ] {
            let j = json(&render(&e, &cx()));
            let d = j["description"].as_str().unwrap();
            assert!(d.contains(&marked), "{e}: {d}");
        }
        let person = ModLogEvent::ServerMute {
            target: t(),
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
            buyer: a(),
            target: t(),
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
            buyer: a(),
            target: t(),
            note: None,
            signed: true,
            until: None,
            mirrored_from: Some(t()),
        };
        let j = json(&render(&bounced, &cx()));
        assert_eq!(
            j["description"],
            "<@22> used Rename on <@11> (bounced by <@11>'s mirror) (signed)"
        );
    }

    #[test]
    fn crowdfund_started_names_the_starter() {
        let e = ModLogEvent::CrowdfundStarted {
            action: ActionKind::Rename,
            starter: a(),
            target: t(),
            note: Some("gg".into()),
            signed: true,
        };
        let j = json(&render(&e, &cx()));
        assert_eq!(j["title"], "🎉 Crowdfund started");
        assert_eq!(
            j["description"],
            "<@22> started a crowdfund for Rename on <@11> (signed)"
        );
        assert_eq!(j["fields"][0]["value"], "gg");
        assert_eq!(render(&e, &cx()).mentions, Mentions::None);
        assert_eq!(e.to_string(), "ModLog(crowdfund_started)");
        // Unsigned, with no note: no reason field, and the starter is still
        // named, marked as anonymous on the public card.
        let bare = ModLogEvent::CrowdfundStarted {
            action: ActionKind::Mute,
            starter: a(),
            target: t(),
            note: None,
            signed: false,
        };
        let j = json(&render(&bare, &cx()));
        assert_eq!(
            j["description"],
            "<@22> started a crowdfund for Mute on <@11> (anonymous)"
        );
        assert!(j["fields"].as_array().is_none_or(Vec::is_empty), "{j}");
    }

    #[test]
    fn crowdfund_executed_renders_every_contributor() {
        let until = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        let c = UserId::new(33);
        let e = ModLogEvent::CrowdfundExecuted {
            action: ActionKind::Rename,
            target: t(),
            contributors: vec![a(), c.to_ref(), UserId::new(44).to_ref()],
            until: Some(until),
        };
        let j = json(&render(&e, &cx()));
        assert_eq!(j["title"], "🎉 Crowdfund executed");
        assert_eq!(
            j["description"],
            "Rename on <@11> until <t:1800000000:f> (crowdfunded by <@22>, <@33>, <@44>)"
        );
        assert_eq!(render(&e, &cx()).mentions, Mentions::None);
        assert_eq!(e.to_string(), "ModLog(crowdfund_executed)");
    }

    #[test]
    fn room_kick_shows_actor_target_room_and_reason() {
        let r = render(
            &ModLogEvent::RoomKick {
                target: t(),
                room: r(),
                actor: a(),
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
                target: t(),
                from: r(),
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
                target: t(),
                room: r(),
                actor: a(),
                reason: "@everyone **hi**".into(),
            },
            &cx(),
        );
        let v = json(&r)["fields"][0]["value"].as_str().unwrap().to_owned();
        assert_eq!(v, "\\@everyone \\*\\*hi\\*\\*");
        let long = render(
            &ModLogEvent::ServerMute {
                target: t(),
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
                by: a(),
                target: t(),
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
                by: a(),
                target: t(),
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
        let r = render(
            &ModLogEvent::ImmunityLifted {
                by: a(),
                target: t(),
            },
            &cx(),
        );
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
                by: a(),
                target: t(),
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
                by: a(),
                target: t(),
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
        let shown: Vec<String> = all().iter().rev().take(7).map(|e| e.to_string()).collect();
        assert_eq!(
            shown,
            [
                "ModLog(crowdfund_executed)",
                "ModLog(crowdfund_started)",
                "ModLog(shop_cancelled)",
                "ModLog(shop_action)",
                "ModLog(credits_adjusted)",
                "ModLog(immunity_lifted)",
                "ModLog(immunity_granted)"
            ]
        );
    }

    /// Captured at e9a778fb, before the events went neutral.
    const EXPECTED: &[&str] = &[
        r##"None|{"title":"🔇 Server muted","type":"rich","description":"<@22> muted <@11>","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"loud","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🔈 Server unmuted","type":"rich","description":"<@22> unmuted <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🙉 Server deafened","type":"rich","description":"<@22> deafened <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"👂 Server undeafened","type":"rich","description":"<@22> undeafened <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"⏏️ Disconnected","type":"rich","description":"<@11> from <#333> by <@22>","timestamp":"2023-11-14T22:13:20Z","footer":{"text":"Matched from Discord's audit log by timing","icon_url":null}}|[]|[]|None"##,
        r##"None|{"title":"↪️ Moved","type":"rich","description":"<@11> from <#333> to <#444> by one of <@22>, <@11>","timestamp":"2023-11-14T22:13:20Z","footer":{"text":"Matched from Discord's audit log by timing","icon_url":null}}|[]|[]|None"##,
        r##"None|{"title":"🗂️ Disconnected several","type":"rich","description":"<@22> disconnected 2 of: <@1001>, <@1002>, <@1003>","timestamp":"2023-11-14T22:13:20Z","footer":{"text":"Matched from Discord's audit log by timing","icon_url":null}}|[]|[]|None"##,
        r##"None|{"title":"👢 Kicked from a room","type":"rich","description":"<@22> kicked <@11> from <#333>","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"spam","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"✅ Let back into a room","type":"rich","description":"<@22> let back in <@11> in <#333>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🤐 Muted in a room","type":"rich","description":"<@22> muted <@11> in <#333>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🗣️ Unmuted in a room","type":"rich","description":"<@22> unmuted <@11> in <#333>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🛡️ Immunity granted","type":"rich","description":"<@22> granted immunity to <@11> until lifted","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🛡️ Immunity lifted","type":"rich","description":"<@22> lifted the immunity of <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🪙 Credits adjusted","type":"rich","description":"<@22> adjusted the credits of <@11> by +5","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"x","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🛍️ Shop action","type":"rich","description":"<@22> used Rename on <@11> (anonymous)","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"hi","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🧼 Shop pranks ended","type":"rich","description":"<@22> ended the pranks on <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🎉 Crowdfund started","type":"rich","description":"<@22> started a crowdfund for Mute on <@11> (anonymous)","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🎉 Crowdfund executed","type":"rich","description":"Mute on <@11> (crowdfunded by <@22>, <@11>)","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🔇 Server muted","type":"rich","description":"<@22> (bot) muted <@11>","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"⏏️ Disconnected","type":"rich","description":"<@11> from <#333> by <@22> (bot)","timestamp":"2023-11-14T22:13:20Z","footer":{"text":"Matched from Discord's audit log by timing","icon_url":null}}|[]|[]|None"##,
        r##"None|{"title":"🗂️ Disconnected several","type":"rich","description":"One or more of <@22>, <@11> (bot) disconnected 14 of: <@1001>, <@1002>, <@1003>, <@1004>, <@1005>, <@1006>, <@1007>, <@1008>, <@1009>, <@1010> and 4 more","timestamp":"2023-11-14T22:13:20Z","footer":{"text":"Matched from Discord's audit log by timing","icon_url":null}}|[]|[]|None"##,
        r##"None|{"title":"👢 Kicked from a room","type":"rich","description":"<@22> kicked <@11> from <#333>","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"\\@everyone \\*x\\*","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🤐 Muted in a room","type":"rich","description":"<@22> muted <@11> in <#333>","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"shh","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🛡️ Immunity granted","type":"rich","description":"<@22> granted immunity to <@11> until <t:1700003600:f>","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"ok","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🪙 Credits adjusted","type":"rich","description":"<@22> adjusted the credits of <@11> by −5","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"x","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🛍️ Shop action","type":"rich","description":"<@22> used Mute on <@11> until <t:1700003600:f> (bounced by <@555>'s mirror) (signed)","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
        r##"None|{"title":"🎉 Crowdfund started","type":"rich","description":"<@22> started a crowdfund for Mute on <@11> (signed)","timestamp":"2023-11-14T22:13:20Z","fields":[{"name":"Reason","value":"n","inline":false}]}|[]|[]|None"##,
        r##"None|{"title":"🎉 Crowdfund executed","type":"rich","description":"Mute on <@11> until <t:1700003600:f> (crowdfunded by <@22>, <@11>)","timestamp":"2023-11-14T22:13:20Z"}|[]|[]|None"##,
    ];
}
