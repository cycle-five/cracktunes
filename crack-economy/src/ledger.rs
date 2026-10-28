//! The economy's own public log.
//!
//! Every line is written as if the whole server reads it, so a hidden
//! (unsigned, unrevealed) buyer's id is unreachable from any render: a
//! [`Byline`] stores no id when hidden, and `Byline::new` is its only
//! constructor. Lines are plain text and never ping.

use super::settings::ActionKind;
use super::EconomyData;
use chrono::{DateTime, Utc};
use crack_common::courier::post_to_channel;
use crack_common::format::escape;
use crack_common::messages::{
    ECONLOG_ADJUSTED, ECONLOG_ANONYMOUS_BACKER, ECONLOG_ANONYMOUS_BACKERS, ECONLOG_A_MODERATOR,
    ECONLOG_BACKED_BY, ECONLOG_BALANCE_BY, ECONLOG_BOUNCED_OFF, ECONLOG_CROWDFUND_LANDED,
    ECONLOG_ENDED_THE_PRANKS_ON, ECONLOG_ENDED_THE_PRANKS_ON_THEM, ECONLOG_IMMUNE,
    ECONLOG_IMMUNITY, ECONLOG_LIFTED, ECONLOG_MADE, ECONLOG_MIRROR, ECONLOG_ON, ECONLOG_OTHER,
    ECONLOG_OTHERS, ECONLOG_STARTED_A_CROWDFUND, ECONLOG_UNMASKED_THE, ECONLOG_UNTIL,
    ECONLOG_UNTIL_LIFTED, ECONLOG_USED, ECONOMY_CARD_SOMEONE, MODLOG_MINUS,
};
use crack_common::render::Rendered;
use crack_common::transport::Transport;
use serenity::all::{GenericChannelId, GuildId, UserId};

/// Who did something, as the log may show them: a user the person chose to
/// sign, or nobody. A hidden byline holds no id at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Byline(Option<UserId>);

impl Byline {
    /// `shown` is whether the person signed (or was unmasked).
    pub fn new(user: UserId, shown: bool) -> Self {
        Self(shown.then_some(user))
    }

    pub fn shown(&self) -> Option<UserId> {
        self.0
    }
}

/// Who ended a target's pranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ender {
    Target,
    Moderator,
}

/// Who unmasked a prank's buyer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmasker {
    Target,
    Moderator,
}

/// One thing the economy log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EconomyLogEvent {
    Purchase {
        action: ActionKind,
        buyer: Byline,
        target: UserId,
        note: Option<String>,
        until: Option<DateTime<Utc>>,
    },
    /// `owner` is whose mirror it hit.
    Bounced {
        action: ActionKind,
        buyer: Byline,
        owner: UserId,
    },
    CrowdfundStarted {
        action: ActionKind,
        starter: Byline,
        target: UserId,
        note: Option<String>,
    },
    CrowdfundLanded {
        action: ActionKind,
        target: UserId,
        backers: Vec<Byline>,
        until: Option<DateTime<Utc>>,
    },
    PranksEnded {
        target: UserId,
        by: Ender,
    },
    ImmunityGranted {
        target: UserId,
        until: Option<DateTime<Utc>>,
        reason: Option<String>,
    },
    ImmunityLifted {
        target: UserId,
    },
    CreditsAdjusted {
        target: UserId,
        amount: i64,
        reason: String,
    },
    Unmasked {
        action: ActionKind,
        target: UserId,
        buyer: UserId,
        by: Unmasker,
    },
}

fn mention(u: UserId) -> String {
    format!("<@{u}>")
}

fn who(b: &Byline) -> String {
    b.shown()
        .map_or_else(|| ECONOMY_CARD_SOMEONE.to_owned(), mention)
}

fn until_text(until: Option<DateTime<Utc>>) -> String {
    until.map_or_else(String::new, |t| {
        format!(" {ECONLOG_UNTIL} <t:{}:f>", t.timestamp())
    })
}

fn with_note(mut text: String, note: Option<&str>) -> String {
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        text.push_str("\n> ");
        text.push_str(&escape(note));
    }
    text
}

fn backers_text(backers: &[Byline]) -> String {
    let shown: Vec<String> = backers
        .iter()
        .filter_map(Byline::shown)
        .map(mention)
        .collect();
    let hidden = backers.len() - shown.len();
    if shown.is_empty() {
        let noun = if hidden == 1 {
            ECONLOG_ANONYMOUS_BACKER
        } else {
            ECONLOG_ANONYMOUS_BACKERS
        };
        return format!("{hidden} {noun}");
    }
    let mut text = shown.join(", ");
    match hidden {
        0 => {},
        1 => text.push_str(&format!(" +1 {ECONLOG_OTHER}")),
        n => text.push_str(&format!(" +{n} {ECONLOG_OTHERS}")),
    }
    text
}

/// The line for `e`, plain text.
pub fn line(e: &EconomyLogEvent) -> String {
    use EconomyLogEvent::{
        Bounced, CreditsAdjusted, CrowdfundLanded, CrowdfundStarted, ImmunityGranted,
        ImmunityLifted, PranksEnded, Purchase, Unmasked,
    };
    match e {
        Purchase {
            action,
            buyer,
            target,
            note,
            until,
        } => with_note(
            format!(
                "{} {ECONLOG_USED} {} {ECONLOG_ON} {}{}",
                who(buyer),
                action.label(),
                mention(*target),
                until_text(*until)
            ),
            note.as_deref(),
        ),
        Bounced {
            action,
            buyer,
            owner,
        } => format!(
            "{}'s {} {ECONLOG_BOUNCED_OFF} {}'s {ECONLOG_MIRROR}",
            who(buyer),
            action.label().to_lowercase(),
            mention(*owner)
        ),
        CrowdfundStarted {
            action,
            starter,
            target,
            note,
        } => with_note(
            format!(
                "{} {ECONLOG_STARTED_A_CROWDFUND} {} {ECONLOG_ON} {}",
                who(starter),
                action.label(),
                mention(*target)
            ),
            note.as_deref(),
        ),
        CrowdfundLanded {
            action,
            target,
            backers,
            until,
        } => format!(
            "{ECONLOG_CROWDFUND_LANDED} {} {ECONLOG_ON} {}{}, {ECONLOG_BACKED_BY} {}",
            action.label(),
            mention(*target),
            until_text(*until),
            backers_text(backers)
        ),
        PranksEnded { target, by } => match by {
            Ender::Target => format!("{} {ECONLOG_ENDED_THE_PRANKS_ON_THEM}", mention(*target)),
            Ender::Moderator => format!(
                "{ECONLOG_A_MODERATOR} {ECONLOG_ENDED_THE_PRANKS_ON} {}",
                mention(*target)
            ),
        },
        ImmunityGranted {
            target,
            until,
            reason,
        } => {
            let when = until.map_or_else(
                || ECONLOG_UNTIL_LIFTED.to_owned(),
                |t| format!("{ECONLOG_UNTIL} <t:{}:f>", t.timestamp()),
            );
            with_note(
                format!(
                    "{ECONLOG_A_MODERATOR} {ECONLOG_MADE} {} {ECONLOG_IMMUNE} {when}",
                    mention(*target)
                ),
                reason.as_deref(),
            )
        },
        ImmunityLifted { target } => format!(
            "{ECONLOG_A_MODERATOR} {ECONLOG_LIFTED} {}'s {ECONLOG_IMMUNITY}",
            mention(*target)
        ),
        CreditsAdjusted {
            target,
            amount,
            reason,
        } => {
            let signed = if *amount < 0 {
                format!("{MODLOG_MINUS}{}", amount.unsigned_abs())
            } else {
                format!("+{amount}")
            };
            with_note(
                format!(
                    "{ECONLOG_A_MODERATOR} {ECONLOG_ADJUSTED} {}'s {ECONLOG_BALANCE_BY} {signed}",
                    mention(*target)
                ),
                Some(reason),
            )
        },
        Unmasked {
            action,
            target,
            buyer,
            by,
        } => {
            let label = action.label().to_lowercase();
            match by {
                Unmasker::Target => format!(
                    "{} {ECONLOG_UNMASKED_THE} {label}: {}",
                    mention(*target),
                    mention(*buyer)
                ),
                Unmasker::Moderator => format!(
                    "{ECONLOG_A_MODERATOR} {ECONLOG_UNMASKED_THE} {label} {ECONLOG_ON} {}: {}",
                    mention(*target),
                    mention(*buyer)
                ),
            }
        },
    }
}

/// The line as a message: plain text that pings nobody.
pub fn render(e: &EconomyLogEvent) -> Rendered {
    Rendered::text(line(e))
}

/// Post `e` to the server's economy log channel. Nothing happens with no
/// channel set; a failed post is logged and changes nothing else.
pub async fn post(data: &EconomyData, transport: &dyn Transport, g: GuildId, e: EconomyLogEvent) {
    let Some(channel) = data.economy.settings(g).config.log_channel_id else {
        tracing::debug!("no economy log channel for {g}; dropping a line");
        return;
    };
    post_to_channel(transport, GenericChannelId::from(channel), render(&e)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{EconomyConfig, EconomySettings};
    use chrono::TimeZone;
    use crack_common::render::Mentions;
    use crack_common::test_support::FakeTransport;
    use crack_common::transport::TransportError;
    use serenity::all::ChannelId;

    const B: UserId = UserId::new(987_654_321);
    const T: UserId = UserId::new(11);
    const O: UserId = UserId::new(12);
    const K: UserId = UserId::new(13);
    const G: GuildId = GuildId::new(1);

    fn at() -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000, 0).unwrap()
    }

    fn hidden() -> Byline {
        Byline::new(B, false)
    }

    fn purchase(
        buyer: Byline,
        note: Option<&str>,
        until: Option<DateTime<Utc>>,
    ) -> EconomyLogEvent {
        EconomyLogEvent::Purchase {
            action: ActionKind::Rename,
            buyer,
            target: T,
            note: note.map(str::to_owned),
            until,
        }
    }

    fn landed(backers: Vec<Byline>) -> EconomyLogEvent {
        EconomyLogEvent::CrowdfundLanded {
            action: ActionKind::Mute,
            target: T,
            backers,
            until: None,
        }
    }

    #[test]
    fn every_line_reads_exactly() {
        use EconomyLogEvent::*;
        let shown = |u| Byline::new(u, true);
        // Purchase: signed or not, until set or not, note set or blank.
        assert_eq!(
            line(&purchase(hidden(), None, None)),
            "Someone used Rename on <@11>"
        );
        assert_eq!(
            line(&purchase(shown(K), None, Some(at()))),
            "<@13> used Rename on <@11> until <t:1800000000:f>"
        );
        assert_eq!(
            line(&purchase(hidden(), Some("hi *there*"), None)),
            "Someone used Rename on <@11>\n> hi \\*there\\*"
        );
        assert_eq!(
            line(&purchase(hidden(), Some("  "), None)),
            "Someone used Rename on <@11>"
        );
        // Bounced.
        assert_eq!(
            line(&Bounced {
                action: ActionKind::Rename,
                buyer: hidden(),
                owner: O
            }),
            "Someone's rename bounced off <@12>'s mirror"
        );
        assert_eq!(
            line(&Bounced {
                action: ActionKind::Rename,
                buyer: shown(K),
                owner: O
            }),
            "<@13>'s rename bounced off <@12>'s mirror"
        );
        // Crowdfund started.
        assert_eq!(
            line(&CrowdfundStarted {
                action: ActionKind::Mute,
                starter: hidden(),
                target: T,
                note: None
            }),
            "Someone started a crowdfund: Mute on <@11>"
        );
        assert_eq!(
            line(&CrowdfundStarted {
                action: ActionKind::Mute,
                starter: shown(K),
                target: T,
                note: Some("go".into())
            }),
            "<@13> started a crowdfund: Mute on <@11>\n> go"
        );
        // Crowdfund landed: backers.
        assert_eq!(
            line(&landed(vec![shown(K), shown(O)])),
            "Crowdfund landed: Mute on <@11>, backed by <@13>, <@12>"
        );
        assert_eq!(
            line(&landed(vec![shown(K), hidden()])),
            "Crowdfund landed: Mute on <@11>, backed by <@13> +1 other"
        );
        assert_eq!(
            line(&landed(vec![shown(K), hidden(), hidden()])),
            "Crowdfund landed: Mute on <@11>, backed by <@13> +2 others"
        );
        assert_eq!(
            line(&landed(vec![hidden()])),
            "Crowdfund landed: Mute on <@11>, backed by 1 anonymous backer"
        );
        assert_eq!(
            line(&landed(vec![hidden(), hidden(), hidden()])),
            "Crowdfund landed: Mute on <@11>, backed by 3 anonymous backers"
        );
        assert_eq!(
            line(&CrowdfundLanded {
                action: ActionKind::Mute,
                target: T,
                backers: vec![shown(K)],
                until: Some(at())
            }),
            "Crowdfund landed: Mute on <@11> until <t:1800000000:f>, backed by <@13>"
        );
        // Pranks ended.
        assert_eq!(
            line(&PranksEnded {
                target: T,
                by: Ender::Target
            }),
            "<@11> ended the pranks on them"
        );
        assert_eq!(
            line(&PranksEnded {
                target: T,
                by: Ender::Moderator
            }),
            "A moderator ended the pranks on <@11>"
        );
        // Immunity.
        assert_eq!(
            line(&ImmunityGranted {
                target: T,
                until: Some(at()),
                reason: None
            }),
            "A moderator made <@11> immune until <t:1800000000:f>"
        );
        assert_eq!(
            line(&ImmunityGranted {
                target: T,
                until: None,
                reason: Some("ok".into())
            }),
            "A moderator made <@11> immune until lifted\n> ok"
        );
        assert_eq!(
            line(&ImmunityLifted { target: T }),
            "A moderator lifted <@11>'s immunity"
        );
        // Credits.
        assert_eq!(
            line(&CreditsAdjusted {
                target: T,
                amount: 5,
                reason: "  ".into()
            }),
            "A moderator adjusted <@11>'s balance by +5"
        );
        assert_eq!(
            line(&CreditsAdjusted {
                target: T,
                amount: -5,
                reason: "oops".into()
            }),
            "A moderator adjusted <@11>'s balance by \u{2212}5\n> oops"
        );
        // Unmasked.
        assert_eq!(
            line(&Unmasked {
                action: ActionKind::Rename,
                target: T,
                buyer: K,
                by: Unmasker::Target
            }),
            "<@11> unmasked the rename: <@13>"
        );
        assert_eq!(
            line(&Unmasked {
                action: ActionKind::Rename,
                target: T,
                buyer: K,
                by: Unmasker::Moderator
            }),
            "A moderator unmasked the rename on <@11>: <@13>"
        );
    }

    /// Every variant that carries a `Byline`, built from `b`.
    fn bylined(b: impl Fn() -> Byline) -> Vec<EconomyLogEvent> {
        vec![
            purchase(b(), Some("n"), Some(at())),
            EconomyLogEvent::Bounced {
                action: ActionKind::Mute,
                buyer: b(),
                owner: O,
            },
            EconomyLogEvent::CrowdfundStarted {
                action: ActionKind::Mute,
                starter: b(),
                target: T,
                note: None,
            },
            landed(vec![b(), b()]),
        ]
    }

    #[test]
    fn no_render_of_a_hidden_buyer_contains_their_id() {
        for e in bylined(hidden) {
            assert!(!line(&e).contains("987654321"), "{e:?}");
            assert!(!format!("{:?}", render(&e)).contains("987654321"), "{e:?}");
        }
        // The same events, shown, do name them: the check can fail.
        for e in bylined(|| Byline::new(B, true)) {
            assert!(line(&e).contains("987654321"), "{e:?}");
        }
        assert_eq!(hidden().shown(), None);
    }

    #[test]
    fn renders_ping_nobody() {
        use EconomyLogEvent::*;
        let all = bylined(|| Byline::new(B, true)).into_iter().chain([
            PranksEnded {
                target: T,
                by: Ender::Moderator,
            },
            ImmunityGranted {
                target: T,
                until: None,
                reason: None,
            },
            ImmunityLifted { target: T },
            CreditsAdjusted {
                target: T,
                amount: 1,
                reason: String::new(),
            },
            Unmasked {
                action: ActionKind::Rename,
                target: T,
                buyer: K,
                by: Unmasker::Moderator,
            },
        ]);
        for e in all {
            assert_eq!(render(&e).mentions, Mentions::None, "{e:?}");
        }
    }

    fn data(channel: Option<ChannelId>) -> EconomyData {
        let d = EconomyData::default();
        d.economy.set_settings(EconomySettings {
            guild: G,
            enabled: true,
            config: EconomyConfig {
                log_channel_id: channel,
                ..Default::default()
            },
        });
        d
    }

    #[tokio::test]
    async fn posts_to_the_log_channel_only_when_set() {
        let e = purchase(hidden(), None, None);
        let t = FakeTransport::default();
        post(&data(None), &t, G, e.clone()).await;
        assert!(t.texts().is_empty());

        post(&data(Some(ChannelId::new(77))), &t, G, e.clone()).await;
        assert_eq!(t.texts(), vec![line(&e)]);
        assert_eq!(t.ops(), vec![crack_common::test_support::Op::Send(77)]);
    }

    #[tokio::test]
    async fn a_failed_post_is_only_logged() {
        let t = FakeTransport::default();
        t.send_failures
            .lock()
            .unwrap()
            .push_back(TransportError::Other("403".into()));
        post(
            &data(Some(ChannelId::new(77))),
            &t,
            G,
            purchase(hidden(), None, None),
        )
        .await;
        // One attempt, no retry, no other send.
        assert_eq!(t.ops().len(), 1);
    }
}
