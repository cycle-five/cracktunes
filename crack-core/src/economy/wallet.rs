//! Types of the money layer: what a ledger row or a purchase can be, and why
//! a move of credits is refused. The SQL lives in `db::economy`.

use serde::{Deserialize, Serialize};
use serenity::all::ChannelId;

/// Why a wallet changed, stored as text in the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerKind {
    Purchase,
    Refund,
    GiftSent,
    GiftReceived,
    CoinflipStake,
    CoinflipWin,
    Adjust,
    BountyPlaced,
    BountyWon,
    Pledge,
    PledgeRefund,
}

impl LedgerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerKind::Purchase => "purchase",
            LedgerKind::Refund => "refund",
            LedgerKind::GiftSent => "gift_sent",
            LedgerKind::GiftReceived => "gift_received",
            LedgerKind::CoinflipStake => "coinflip_stake",
            LedgerKind::CoinflipWin => "coinflip_win",
            LedgerKind::Adjust => "adjust",
            LedgerKind::BountyPlaced => "bounty_placed",
            LedgerKind::BountyWon => "bounty_won",
            LedgerKind::Pledge => "pledge",
            LedgerKind::PledgeRefund => "pledge_refund",
        }
    }
}

/// Where a purchase is in its life, stored as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Pending,
    /// A crowdfund still filling.
    Funding,
    Active,
    Ending,
    Done,
    Ended,
    Failed,
}

impl ActionState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionState::Pending => "pending",
            ActionState::Funding => "funding",
            ActionState::Active => "active",
            ActionState::Ending => "ending",
            ActionState::Done => "done",
            ActionState::Ended => "ended",
            ActionState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            ActionState::Pending,
            ActionState::Funding,
            ActionState::Active,
            ActionState::Ending,
            ActionState::Done,
            ActionState::Ended,
            ActionState::Failed,
        ]
        .into_iter()
        .find(|st| st.as_str() == s)
    }

    /// True when the action will not change again.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            ActionState::Done | ActionState::Ended | ActionState::Failed
        )
    }
}

/// What a purchase carries, stored as JSONB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionPayload {
    Immunity {
        hours: u32,
    },
    /// `restore` is the nickname to put back (`None` = had no nickname).
    Rename {
        name: String,
        restore: Option<String>,
    },
    Clown {
        applied: String,
        restore: Option<String>,
    },
    /// `locked` is the nickname the lock holds (`None` = no nickname).
    Lock {
        locked: Option<String>,
    },
    Drag {
        from: ChannelId,
        to: ChannelId,
    },
    Mute,
    Deafen,
    Jail {
        jail: ChannelId,
    },
    Mirror,
    Cleanse,
    Reveal {
        action_id: i64,
    },
    /// A walk-on clip for the target's next join. The link is never shown.
    Theme {
        url: String,
        start_secs: u32,
    },
    Dedicate {
        song: String,
    },
    Jump {
        song: String,
    },
    Bounty,
}

/// Why credits did not move.
#[derive(Debug)]
pub enum MoveError {
    /// The debited wallet holds less than the amount; this is what it holds.
    Insufficient {
        balance: i64,
    },
    /// The request is malformed (non-positive amount, gift to oneself, an
    /// overflow); nothing was written.
    Invalid,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for MoveError {
    fn from(e: sqlx::Error) -> Self {
        MoveError::Db(e)
    }
}

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MoveError::Insufficient { balance } => write!(f, "insufficient balance ({balance})"),
            MoveError::Invalid => write!(f, "invalid amount"),
            MoveError::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for MoveError {}

#[cfg(test)]
mod payload_tests {
    use super::*;

    #[test]
    fn action_state_text_round_trips() {
        for st in [
            ActionState::Pending,
            ActionState::Funding,
            ActionState::Active,
            ActionState::Ending,
            ActionState::Done,
            ActionState::Ended,
            ActionState::Failed,
        ] {
            assert_eq!(ActionState::parse(st.as_str()), Some(st));
        }
        assert_eq!(ActionState::parse("funding"), Some(ActionState::Funding));
        assert_eq!(ActionState::parse("nope"), None);
    }

    #[test]
    fn payload_round_trips_every_variant() {
        let ch = |n| ChannelId::new(n);
        let cases = [
            (
                ActionPayload::Immunity { hours: 24 },
                r#"{"kind":"immunity","hours":24}"#,
            ),
            (
                ActionPayload::Rename {
                    name: "Bob".into(),
                    restore: Some("Al".into()),
                },
                r#"{"kind":"rename","name":"Bob","restore":"Al"}"#,
            ),
            (
                ActionPayload::Rename {
                    name: "Bob".into(),
                    restore: None,
                },
                r#"{"kind":"rename","name":"Bob","restore":null}"#,
            ),
            (
                ActionPayload::Clown {
                    applied: "Al 🤡".into(),
                    restore: None,
                },
                r#"{"kind":"clown","applied":"Al 🤡","restore":null}"#,
            ),
            (
                ActionPayload::Lock {
                    locked: Some("Al".into()),
                },
                r#"{"kind":"lock","locked":"Al"}"#,
            ),
            (
                ActionPayload::Drag {
                    from: ch(1),
                    to: ch(2),
                },
                r#"{"kind":"drag","from":"1","to":"2"}"#,
            ),
            (ActionPayload::Mute, r#"{"kind":"mute"}"#),
            (ActionPayload::Deafen, r#"{"kind":"deafen"}"#),
            (
                ActionPayload::Jail { jail: ch(3) },
                r#"{"kind":"jail","jail":"3"}"#,
            ),
            (ActionPayload::Mirror, r#"{"kind":"mirror"}"#),
            (ActionPayload::Cleanse, r#"{"kind":"cleanse"}"#),
            (
                ActionPayload::Reveal { action_id: 42 },
                r#"{"kind":"reveal","action_id":42}"#,
            ),
            (
                ActionPayload::Theme {
                    url: "https://x".into(),
                    start_secs: 83,
                },
                r#"{"kind":"theme","url":"https://x","start_secs":83}"#,
            ),
            (
                ActionPayload::Dedicate { song: "s".into() },
                r#"{"kind":"dedicate","song":"s"}"#,
            ),
            (
                ActionPayload::Jump { song: "j".into() },
                r#"{"kind":"jump","song":"j"}"#,
            ),
            (ActionPayload::Bounty, r#"{"kind":"bounty"}"#),
        ];
        for (payload, wire) in cases {
            assert_eq!(serde_json::to_string(&payload).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<ActionPayload>(wire).unwrap(),
                payload
            );
        }
    }
}
