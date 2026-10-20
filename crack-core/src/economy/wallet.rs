//! Types of the money layer: what a ledger row or a purchase can be, and why
//! a move of credits is refused. The SQL lives in `db::economy`.

use serde::{Deserialize, Serialize};

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
        }
    }
}

/// Where a purchase is in its life, stored as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    Pending,
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
            ActionState::Active => "active",
            ActionState::Ending => "ending",
            ActionState::Done => "done",
            ActionState::Ended => "ended",
            ActionState::Failed => "failed",
        }
    }

    /// True when the action will not change again.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            ActionState::Done | ActionState::Ended | ActionState::Failed
        )
    }
}

/// What a purchase carries, stored as JSONB. PR 2 adds variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionPayload {
    Immunity { hours: u32 },
}

/// Why credits did not move.
#[derive(Debug)]
pub enum MoveError {
    /// The debited wallet holds less than the amount; this is what it holds.
    Insufficient {
        balance: i64,
    },
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
            MoveError::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for MoveError {}
