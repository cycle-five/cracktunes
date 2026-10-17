//! The events the mod log posts, and the one embed each renders to.
use crate::messaging::format::{clip, escape, DESCRIPTION_MAX, EMBED_TITLE_MAX, FIELD_MAX};
use crate::messaging::messages::*;
use crate::messaging::render::{Mentions, RenderCx, Rendered};
use serenity::all::{ChannelId, CreateEmbed, CreateEmbedFooter, Timestamp, UserId};

/// How many members a batch names before "and N more".
const BATCH_NAMES_MAX: usize = 10;

/// Who a forced disconnect or move is credited to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    By(UserId),
    OneOf(Vec<UserId>),
}

/// One thing the mod log reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModLogEvent {
    ServerMute {
        target: UserId,
        actor: UserId,
        on: bool,
        reason: Option<String>,
    },
    ServerDeafen {
        target: UserId,
        actor: UserId,
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
        actors: Vec<UserId>,
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

fn by(a: &Attribution) -> String {
    match a {
        Attribution::By(u) => format!("{MODLOG_BY} {}", user(*u)),
        Attribution::OneOf(l) => format!("{MODLOG_BY_ONE_OF} {}", users(l)),
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
                user(*actor),
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
                user(*actor),
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
            let who = match actors.as_slice() {
                [_, _, ..] => format!("{MODLOG_ONE_OR_MORE_OF} {}", users(actors)),
                _ => users(actors),
            };
            Parts {
                audit: true,
                ..plain(
                    MODLOG_TITLE_BATCH,
                    format!("{who} {MODLOG_DISCONNECTED} {count} {MODLOG_OF}: {shown}{more}"),
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
                actor: A,
                on: true,
                reason: Some("loud".into()),
            },
            ModLogEvent::ServerMute {
                target: T,
                actor: A,
                on: false,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: T,
                actor: A,
                on: true,
                reason: None,
            },
            ModLogEvent::ServerDeafen {
                target: T,
                actor: A,
                on: false,
                reason: None,
            },
            ModLogEvent::Disconnect {
                target: T,
                from: R,
                by: Attribution::By(A),
            },
            ModLogEvent::Move {
                target: T,
                from: R,
                to: R2,
                by: Attribution::OneOf(vec![A, T]),
            },
            ModLogEvent::DisconnectBatch {
                actors: vec![A],
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
                by: Attribution::OneOf(vec![UserId::new(5), UserId::new(6), UserId::new(7)]),
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
                    actors: vec![A],
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
                actors: vec![UserId::new(5), UserId::new(6)],
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
                actor: A,
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
}
