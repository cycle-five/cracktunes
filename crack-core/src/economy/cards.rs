//! The public card a prank posts, and where it goes. Pure: the caller sends.

use super::settings::ActionKind;
use super::shop::BuyOutcome;
use crate::messaging::format::escape;
use crate::messaging::messages::{
    ECONOMY_BOUNTY_SIGN_ANON, ECONOMY_BOUNTY_SIGN_DASH, ECONOMY_THEME_CARD_PREFIX,
    ECONOMY_THEME_CARD_SUFFIX,
};
use crate::messaging::messages::{
    ECONOMY_CARD_BACK_AT_THEM, ECONOMY_CARD_BLOCKED_TAIL, ECONOMY_CARD_CLOWNED,
    ECONOMY_CARD_DEAFENED, ECONOMY_CARD_DRAGGED, ECONOMY_CARD_DRAGGED_TAIL, ECONOMY_CARD_JAILED,
    ECONOMY_CARD_LOCKED, ECONOMY_CARD_MIRROR, ECONOMY_CARD_MIRROR_BOUNCED, ECONOMY_CARD_MUTED,
    ECONOMY_CARD_POSSESSIVE, ECONOMY_CARD_RENAMED, ECONOMY_CARD_SOMEONE, ECONOMY_CARD_TO,
    ECONOMY_CARD_UNTIL, ECONOMY_FUND_CARD_AND, ECONOMY_FUND_CARD_CROWD, ECONOMY_FUND_CARD_OTHER,
    ECONOMY_FUND_CARD_OTHERS, ECONOMY_FUND_CARD_PREFIX, ECONOMY_FUND_CARD_TO,
    ECONOMY_FUND_CARD_UNTIL, ECONOMY_FUND_FAILED_CARD_MIDDLE, ECONOMY_FUND_FAILED_CARD_PREFIX,
    ECONOMY_FUND_FAILED_CARD_SUFFIX, ECONOMY_FUND_START_CARD_STARTED, ECONOMY_FUND_START_CARD_TAIL,
    ECONOMY_FUND_TO_CLOWN, ECONOMY_FUND_TO_DEAFEN, ECONOMY_FUND_TO_JAIL, ECONOMY_FUND_TO_LOCK,
    ECONOMY_FUND_TO_MUTE, ECONOMY_FUND_TO_RENAME,
};
use chrono::{DateTime, Utc};
use serenity::all::{ChannelId, UserId};

fn mention(u: UserId) -> String {
    format!("<@{u}>")
}

fn until_text(until: Option<DateTime<Utc>>) -> String {
    until.map_or_else(String::new, |t| {
        format!(" {ECONOMY_CARD_UNTIL} <t:{}:R>", t.timestamp())
    })
}

/// The card for a prank of `kind` aimed at `target`.
///
/// `buyer` is `None` for an unsigned prank, which reads "Someone". A bounce
/// keeps that: the card says whose mirror it hit and what happened, and names
/// the buyer only when the buyer signed. Kinds that are not pranks (mirror,
/// cleanse, reveal, immunity) have no card and return an empty string.
pub fn card_text(
    kind: ActionKind,
    target: UserId,
    nick: Option<&str>,
    until: Option<DateTime<Utc>>,
    note: Option<&str>,
    buyer: Option<UserId>,
    outcome: BuyOutcome,
) -> String {
    if !kind.is_prank() {
        return String::new();
    }
    let who = buyer.map_or_else(|| ECONOMY_CARD_SOMEONE.to_string(), mention);
    let t = mention(target);
    let mut text = match outcome {
        BuyOutcome::Landed => landed(kind, &who, &t, nick, until),
        BuyOutcome::Bounced => format!(
            "{ECONOMY_CARD_MIRROR} {t}{ECONOMY_CARD_POSSESSIVE} {ECONOMY_CARD_MIRROR_BOUNCED} {who}{ECONOMY_CARD_POSSESSIVE} {} {ECONOMY_CARD_BACK_AT_THEM}{}.",
            kind.label().to_lowercase(),
            until_text(until)
        ),
        BuyOutcome::Blocked => format!(
            "{ECONOMY_CARD_MIRROR} {t}{ECONOMY_CARD_POSSESSIVE} {ECONOMY_CARD_MIRROR_BOUNCED} {who}{ECONOMY_CARD_POSSESSIVE} {}, {ECONOMY_CARD_BLOCKED_TAIL}",
            kind.label().to_lowercase()
        ),
    };
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        text.push_str("\n> ");
        text.push_str(&escape(note));
    }
    text
}

fn landed(
    kind: ActionKind,
    who: &str,
    t: &str,
    nick: Option<&str>,
    until: Option<DateTime<Utc>>,
) -> String {
    let until = until_text(until);
    match kind {
        ActionKind::Rename => format!(
            "{who} {ECONOMY_CARD_RENAMED} {t} {ECONOMY_CARD_TO} **{}**{until}",
            escape(nick.unwrap_or_default())
        ),
        ActionKind::Lock => format!("{who} {ECONOMY_CARD_LOCKED} {t}{until}"),
        ActionKind::Clown => format!("{who} {ECONOMY_CARD_CLOWNED} {t}{until}"),
        ActionKind::Drag => {
            format!("{who} {ECONOMY_CARD_DRAGGED} {t} {ECONOMY_CARD_DRAGGED_TAIL}")
        },
        ActionKind::Mute => format!("{who} {ECONOMY_CARD_MUTED} {t}{until}"),
        ActionKind::Deafen => format!("{who} {ECONOMY_CARD_DEAFENED} {t}{until}"),
        ActionKind::Jail => format!("{who} {ECONOMY_CARD_JAILED} {t}{until}"),
        // Not pranks: `card_text` returns before here.
        _ => String::new(),
    }
}

/// The credit line of a crowdfund card: the signed contributors, "a crowd"
/// when nobody signed. Two are named; more than two read "@a, @b and N
/// others".
fn fund_signoff(signers: &[UserId]) -> String {
    match signers {
        [] => ECONOMY_FUND_CARD_CROWD.to_string(),
        [a] => mention(*a),
        [a, b] => format!("{} {ECONOMY_FUND_CARD_AND} {}", mention(*a), mention(*b)),
        [a, b, rest @ ..] => {
            let n = rest.len();
            let word = if n == 1 {
                ECONOMY_FUND_CARD_OTHER
            } else {
                ECONOMY_FUND_CARD_OTHERS
            };
            format!(
                "{}, {} {ECONOMY_FUND_CARD_AND} {n} {word}",
                mention(*a),
                mention(*b)
            )
        },
    }
}

/// What a crowdfunded prank does to `target`, in words.
fn fund_deed(kind: ActionKind, target: UserId, nick: Option<&str>) -> Option<String> {
    let t = mention(target);
    Some(match kind {
        ActionKind::Rename => format!(
            "{ECONOMY_CARD_RENAMED} {t} {ECONOMY_FUND_CARD_TO} **{}**",
            escape(nick.unwrap_or_default())
        ),
        ActionKind::Lock => format!("{ECONOMY_CARD_LOCKED} {t}"),
        ActionKind::Clown => format!("{ECONOMY_CARD_CLOWNED} {t}"),
        ActionKind::Mute => format!("{ECONOMY_CARD_MUTED} {t}"),
        ActionKind::Deafen => format!("{ECONOMY_CARD_DEAFENED} {t}"),
        ActionKind::Jail => format!("{ECONOMY_CARD_JAILED} {t}"),
        // Not fundable: `/crowdfund` never opens one.
        _ => return None,
    })
}

/// The card for a crowdfund that landed: what was done to `target`, the note,
/// then the signed contributors (never an unsigned one).
pub fn fund_card_text(
    kind: ActionKind,
    target: UserId,
    nick: Option<&str>,
    until: Option<DateTime<Utc>>,
    note: Option<&str>,
    signers: &[UserId],
) -> String {
    let Some(did) = fund_deed(kind, target, nick) else {
        return String::new();
    };
    let mut text = format!("{ECONOMY_FUND_CARD_PREFIX} {did}");
    if let Some(until) = until {
        text.push_str(&format!(
            " {ECONOMY_FUND_CARD_UNTIL} <t:{}:f>",
            until.timestamp()
        ));
    }
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        text.push_str("\n> ");
        text.push_str(&escape(note));
    }
    text.push_str(&format!("\n\u{2014} {}", fund_signoff(signers)));
    text
}

/// What a crowdfunded prank would do to `target`, as an infinitive ("to
/// mute @t"), for the card announcing a new fund.
fn fund_intent(kind: ActionKind, target: UserId, nick: Option<&str>) -> Option<String> {
    let t = mention(target);
    Some(match kind {
        ActionKind::Rename => format!(
            "{ECONOMY_FUND_TO_RENAME} {t} {ECONOMY_FUND_CARD_TO} **{}**",
            escape(nick.unwrap_or_default())
        ),
        ActionKind::Clown => format!("{ECONOMY_FUND_TO_CLOWN} {t}"),
        ActionKind::Lock => format!("{ECONOMY_FUND_TO_LOCK} {t}"),
        ActionKind::Mute => format!("{ECONOMY_FUND_TO_MUTE} {t}"),
        ActionKind::Deafen => format!("{ECONOMY_FUND_TO_DEAFEN} {t}"),
        ActionKind::Jail => format!("{ECONOMY_FUND_TO_JAIL} {t}"),
        _ => return None,
    })
}

/// The card for a crowdfund that has just opened, so others can chip in. The
/// starter is named only when the fund is signed.
pub fn fund_started_card_text(
    kind: ActionKind,
    target: UserId,
    nick: Option<&str>,
    pledged: i64,
    price: i64,
    currency: &str,
    starter: Option<UserId>,
) -> String {
    let Some(deed) = fund_intent(kind, target, nick) else {
        return String::new();
    };
    let who = starter.map_or_else(|| ECONOMY_CARD_SOMEONE.to_string(), mention);
    format!(
        "\u{1F389} {who} {ECONOMY_FUND_START_CARD_STARTED} {deed}: {pledged}/{price} {currency}. {ECONOMY_FUND_START_CARD_TAIL}"
    )
}

/// The card for a filled crowdfund that could not land.
pub fn fund_failed_card_text(target: UserId, reason: &str) -> String {
    format!(
        "{ECONOMY_FUND_FAILED_CARD_PREFIX} {} {ECONOMY_FUND_FAILED_CARD_MIDDLE} ({reason}). {ECONOMY_FUND_FAILED_CARD_SUFFIX}",
        mention(target)
    )
}

/// The card a walk-on theme posts as it plays: whose theme, the note, then
/// the buyer when they signed ("someone" when not). The link is never on it.
pub fn theme_card_text(target: UserId, note: Option<&str>, buyer: Option<UserId>) -> String {
    let mut text = format!(
        "{ECONOMY_THEME_CARD_PREFIX} {}{ECONOMY_THEME_CARD_SUFFIX}",
        mention(target)
    );
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        text.push_str("\n> ");
        text.push_str(&escape(note));
    }
    let by = buyer.map_or_else(|| ECONOMY_BOUNTY_SIGN_ANON.to_string(), mention);
    text.push_str(&format!("\n{ECONOMY_BOUNTY_SIGN_DASH} {by}"));
    text
}

/// The channel a card goes to. Drag, jail, mute and deafen happen in voice,
/// so they post in the target's voice channel's chat while they are still in
/// one; everything else (and those, once the target has left voice) goes to
/// the announce channel, else where the command ran.
pub fn card_destination(
    kind: ActionKind,
    target_voice: Option<ChannelId>,
    invoked_in: ChannelId,
    announce: Option<ChannelId>,
) -> ChannelId {
    let fallback = announce.unwrap_or(invoked_in);
    match kind {
        ActionKind::Drag | ActionKind::Jail | ActionKind::Mute | ActionKind::Deafen => {
            target_voice.unwrap_or(fallback)
        },
        _ => fallback,
    }
}

/// Where a card for a purchase goes, from what really happened: a landed
/// drag leaves the target in the buyer's channel, a landed or bounced jail in
/// the jail channel, any other bounce on the buyer (so in their channel), and
/// the rest where the target is. Then [`card_destination`] picks voice chat
/// or the fallback.
pub fn card_channel(
    kind: ActionKind,
    outcome: BuyOutcome,
    target_voice: Option<ChannelId>,
    buyer_voice: Option<ChannelId>,
    jail: Option<ChannelId>,
    invoked_in: ChannelId,
    announce: Option<ChannelId>,
) -> ChannelId {
    let voice = match (kind, outcome) {
        (ActionKind::Jail, BuyOutcome::Landed | BuyOutcome::Bounced) => jail,
        (ActionKind::Drag, BuyOutcome::Landed) | (_, BuyOutcome::Bounced) => buyer_voice,
        _ => target_voice,
    };
    card_destination(kind, voice, invoked_in, announce)
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: UserId = UserId::new(10);
    const T: UserId = UserId::new(20);

    fn until() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    #[test]
    fn theme_card_names_the_target_the_note_and_the_signer() {
        assert_eq!(
            theme_card_text(T, Some("for *you*"), Some(B)),
            "🎺 <@20>'s walk-on theme\n> for \\*you\\*\n— <@10>"
        );
        assert_eq!(
            theme_card_text(T, None, None),
            "🎺 <@20>'s walk-on theme\n— someone"
        );
        assert_eq!(
            theme_card_text(T, Some("  "), None),
            "🎺 <@20>'s walk-on theme\n— someone",
            "a blank note is no note"
        );
    }

    #[test]
    fn card_text_signed_and_anonymous() {
        let signed = card_text(
            ActionKind::Rename,
            T,
            Some("Mr. *Bean*"),
            Some(until()),
            Some("gg"),
            Some(B),
            BuyOutcome::Landed,
        );
        assert_eq!(
            signed,
            "<@10> renamed <@20> to **Mr. \\*Bean\\*** until <t:1800000000:R>\n> gg"
        );
        let anon = card_text(
            ActionKind::Rename,
            T,
            Some("Bean"),
            Some(until()),
            None,
            None,
            BuyOutcome::Landed,
        );
        assert_eq!(
            anon,
            "Someone renamed <@20> to **Bean** until <t:1800000000:R>"
        );
        assert!(!anon.contains("<@10>"));
        assert_eq!(
            card_text(
                ActionKind::Drag,
                T,
                None,
                None,
                None,
                None,
                BuyOutcome::Landed
            ),
            "Someone dragged <@20> into their voice channel"
        );
        for k in [
            ActionKind::Lock,
            ActionKind::Clown,
            ActionKind::Mute,
            ActionKind::Deafen,
            ActionKind::Jail,
        ] {
            let c = card_text(k, T, None, Some(until()), None, Some(B), BuyOutcome::Landed);
            assert!(c.starts_with("<@10> ") && c.contains("<@20>"), "{k:?}: {c}");
            assert!(c.ends_with("until <t:1800000000:R>"), "{k:?}: {c}");
        }
        // Not a prank: no card.
        assert_eq!(
            card_text(
                ActionKind::Mirror,
                T,
                None,
                None,
                None,
                Some(B),
                BuyOutcome::Landed
            ),
            ""
        );
    }

    #[test]
    fn a_mirrored_card_says_what_happened() {
        let bounced = card_text(
            ActionKind::Mute,
            T,
            None,
            Some(until()),
            None,
            Some(B),
            BuyOutcome::Bounced,
        );
        assert_eq!(
            bounced,
            "🪞 <@20>'s mirror bounced <@10>'s mute back at them until <t:1800000000:R>."
        );
        let blocked = card_text(
            ActionKind::Drag,
            T,
            None,
            None,
            None,
            None,
            BuyOutcome::Blocked,
        );
        assert_eq!(
            blocked,
            "🪞 <@20>'s mirror bounced Someone's drag, but it couldn't land on them. The credits are spent."
        );
    }

    #[test]
    fn card_destination_per_kind() {
        let voice = ChannelId::new(1);
        let here = ChannelId::new(2);
        let announce = ChannelId::new(3);
        for k in [
            ActionKind::Drag,
            ActionKind::Jail,
            ActionKind::Mute,
            ActionKind::Deafen,
        ] {
            assert_eq!(
                card_destination(k, Some(voice), here, Some(announce)),
                voice,
                "{k:?}"
            );
            // The target already left voice: the usual place.
            assert_eq!(card_destination(k, None, here, Some(announce)), announce);
            assert_eq!(card_destination(k, None, here, None), here);
        }
        for k in [ActionKind::Rename, ActionKind::Lock, ActionKind::Clown] {
            assert_eq!(
                card_destination(k, Some(voice), here, Some(announce)),
                announce,
                "{k:?}"
            );
            assert_eq!(card_destination(k, Some(voice), here, None), here, "{k:?}");
        }
    }

    #[test]
    fn card_channel_follows_where_the_target_ends_up() {
        let (target_vc, my_vc, jail, here, announce) = (
            Some(ChannelId::new(1)),
            Some(ChannelId::new(2)),
            Some(ChannelId::new(3)),
            ChannelId::new(4),
            Some(ChannelId::new(5)),
        );
        let at = |kind, outcome| {
            card_channel(kind, outcome, target_vc, my_vc, jail, here, announce).get()
        };
        use BuyOutcome::*;
        // A landed drag leaves them in the buyer's channel; jail in the jail.
        assert_eq!(at(ActionKind::Drag, Landed), 2);
        assert_eq!(at(ActionKind::Jail, Landed), 3);
        assert_eq!(at(ActionKind::Mute, Landed), 1);
        assert_eq!(at(ActionKind::Deafen, Landed), 1);
        // A bounce lands on the buyer, so it goes where the buyer is.
        assert_eq!(at(ActionKind::Mute, Bounced), 2);
        assert_eq!(at(ActionKind::Jail, Bounced), 3);
        // A blocked drag moved nobody: the target's channel.
        assert_eq!(at(ActionKind::Drag, Blocked), 1);
        // Name pranks never go to voice chat.
        assert_eq!(at(ActionKind::Rename, Landed), 5);
        assert_eq!(at(ActionKind::Rename, Bounced), 5);
        // No announce channel: where the command ran.
        assert_eq!(
            card_channel(
                ActionKind::Clown,
                Landed,
                target_vc,
                my_vc,
                jail,
                here,
                None
            )
            .get(),
            4
        );
        // Out of voice: the fallback.
        assert_eq!(
            card_channel(ActionKind::Mute, Landed, None, my_vc, jail, here, announce).get(),
            5
        );
    }

    #[test]
    fn fund_card_names_only_signers() {
        let d = until();
        let ts = d.timestamp();
        let a = UserId::new(10);
        let b = UserId::new(11);
        let c = UserId::new(12);
        let e = UserId::new(13);
        // Nobody signed: a crowd, and no one is named.
        let crowd = fund_card_text(
            ActionKind::Rename,
            T,
            Some("Bean"),
            Some(d),
            Some("gg"),
            &[],
        );
        assert_eq!(
            crowd,
            format!(
                "🎉 A crowdfund renamed <@20> to **Bean** until <t:{ts}:f>\n> gg\n\u{2014} a crowd"
            )
        );
        let one = fund_card_text(ActionKind::Mute, T, None, Some(d), None, &[a]);
        assert_eq!(
            one,
            format!("🎉 A crowdfund muted <@20> until <t:{ts}:f>\n\u{2014} <@10>")
        );
        let two = fund_card_text(ActionKind::Jail, T, None, Some(d), None, &[a, b]);
        assert!(two.ends_with("\u{2014} <@10> and <@11>"), "{two}");
        let three = fund_card_text(ActionKind::Lock, T, None, Some(d), None, &[a, b, c]);
        assert!(
            three.ends_with("\u{2014} <@10>, <@11> and 1 other"),
            "{three}"
        );
        let four = fund_card_text(ActionKind::Clown, T, None, Some(d), None, &[a, b, c, e]);
        assert!(
            four.ends_with("\u{2014} <@10>, <@11> and 2 others"),
            "{four}"
        );
        assert!(!four.contains("<@12>") && !four.contains("<@13>"));
        // Every fundable kind has words; drag has none.
        for k in ActionKind::FUNDABLE {
            assert!(
                fund_card_text(*k, T, Some("x"), Some(d), None, &[]).starts_with("🎉 A crowdfund "),
                "{k:?}"
            );
        }
        assert_eq!(
            fund_card_text(ActionKind::Drag, T, None, None, None, &[]),
            ""
        );
        // A note's markdown is escaped.
        assert!(
            fund_card_text(ActionKind::Mute, T, None, None, Some("**x**"), &[])
                .contains("\\*\\*x\\*\\*")
        );
        assert_eq!(
            fund_failed_card_text(T, "they aren't in voice"),
            "The crowdfund against <@20> couldn't land (they aren't in voice). Everyone was refunded."
        );
    }

    #[test]
    fn fund_started_card_names_the_starter_only_when_signed() {
        let signed = fund_started_card_text(
            ActionKind::Mute,
            T,
            None,
            30,
            90,
            "credits",
            Some(UserId::new(10)),
        );
        assert_eq!(
            signed,
            "🎉 <@10> started a crowdfund to mute <@20>: 30/90 credits. Chip in with /crowdfund pledge."
        );
        let anon =
            fund_started_card_text(ActionKind::Rename, T, Some("Bean"), 0, 120, "credits", None);
        assert_eq!(
            anon,
            "🎉 Someone started a crowdfund to rename <@20> to **Bean**: 0/120 credits. Chip in with /crowdfund pledge."
        );
        assert!(!anon.contains("<@10>"));
        // The words for every fundable kind.
        let words: Vec<String> = ActionKind::FUNDABLE
            .iter()
            .map(|k| fund_started_card_text(*k, T, Some("x"), 0, 1, "c", None))
            .map(|c| {
                c.split("crowdfund to ")
                    .nth(1)
                    .unwrap()
                    .split(": ")
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            words,
            [
                "rename <@20> to **x**",
                "make a clown of <@20>",
                "lock the nickname of <@20>",
                "mute <@20>",
                "deafen <@20>",
                "jail <@20>"
            ]
        );
        assert_eq!(
            fund_started_card_text(ActionKind::Drag, T, None, 0, 1, "c", None),
            ""
        );
    }
}
