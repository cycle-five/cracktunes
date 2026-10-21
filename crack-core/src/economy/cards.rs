//! The public card a prank posts, and where it goes. Pure: the caller sends.

use super::settings::ActionKind;
use super::shop::BuyOutcome;
use crate::messaging::format::escape;
use crate::messaging::messages::{
    ECONOMY_CARD_BACK_AT_THEM, ECONOMY_CARD_BLOCKED_TAIL, ECONOMY_CARD_CLOWNED,
    ECONOMY_CARD_DEAFENED, ECONOMY_CARD_DRAGGED, ECONOMY_CARD_DRAGGED_TAIL, ECONOMY_CARD_JAILED,
    ECONOMY_CARD_LOCKED, ECONOMY_CARD_MIRROR, ECONOMY_CARD_MIRROR_BOUNCED, ECONOMY_CARD_MUTED,
    ECONOMY_CARD_POSSESSIVE, ECONOMY_CARD_RENAMED, ECONOMY_CARD_SOMEONE, ECONOMY_CARD_TO,
    ECONOMY_CARD_UNTIL,
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
}
