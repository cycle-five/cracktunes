//! The resolver: one pure function, [`decide`], says when a pending withdrawal may end (spec
//! 6.2). The in-command poll and the sweeper both call it, so they cannot disagree.
//!
//! **Nothing refunds without the chain's proof**, apart from the never-sent orphan. An RPC error
//! proves nothing, so it always waits.

use super::chain::{ChainError, ChainStatus, CoinChain};

/// A `NotFound` signature counts as expired only once the finalized height is past its
/// `last_valid_block_height` by more than this many blocks (about a minute). The margin covers a
/// load-balanced RPC pool answering the height and the status from nodes at different heights
/// (Ruling R9).
pub const EXPIRY_MARGIN: u64 = 150;

/// What the database knows about a pending row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowFacts<'a> {
    /// A signature is present (since R13a), so the transaction may have been sent. This is not
    /// about the tx bytes: a row with a signature counts as sent whether or not they are present.
    pub sent: bool,
    /// Created before this process started, or older than `ORPHAN_AFTER_SECS`.
    pub orphan: bool,
    pub last_valid_block_height: Option<i64>,
    pub genesis_hash: Option<&'a str>,
}

/// What the chain said about a sent row, read by [`observe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// A call failed or timed out (HTTP 429 included). Proves nothing.
    Unreachable,
    Seen {
        status: ChainStatus,
        /// Read before `status` (Ruling R9).
        finalized_height: u64,
        genesis: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundReason {
    NeverSent,
    FailedOnChain(String),
    DevnetReset,
    Expired,
    /// Reason text this module did not write (a hand edit). Only [`RefundReason::from_db_text`]
    /// makes one; [`decide`] never does.
    Other(String),
}

const NEVER_SENT: &str = crate::db::economy::coin::NEVER_SENT_REASON;
const FAILED_ON_CHAIN: &str = "failed on chain: ";
const DEVNET_RESET: &str = "devnet was reset";
const EXPIRED: &str = "expired";

impl RefundReason {
    /// The text stored in `coin_withdrawals.reason`.
    pub fn as_db_text(&self) -> String {
        match self {
            RefundReason::NeverSent => NEVER_SENT.to_owned(),
            RefundReason::FailedOnChain(why) => format!("{FAILED_ON_CHAIN}{why}"),
            RefundReason::DevnetReset => DEVNET_RESET.to_owned(),
            RefundReason::Expired => EXPIRED.to_owned(),
            RefundReason::Other(text) => text.clone(),
        }
    }

    /// The inverse of [`RefundReason::as_db_text`]; anything else is [`RefundReason::Other`].
    /// Used to report a row that another resolver ended.
    pub fn from_db_text(text: &str) -> RefundReason {
        match text {
            NEVER_SENT => RefundReason::NeverSent,
            DEVNET_RESET => RefundReason::DevnetReset,
            EXPIRED => RefundReason::Expired,
            _ => match text.strip_prefix(FAILED_ON_CHAIN) {
                Some(why) => RefundReason::FailedOnChain(why.to_owned()),
                None => RefundReason::Other(text.to_owned()),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Done,
    Refund(RefundReason),
    /// Not yet. `resend`: send the stored bytes again (the same signature, so it is idempotent).
    Wait {
        resend: bool,
    },
}

const WAIT: Decision = Decision::Wait { resend: false };

/// Spec 6.2's table. For a row that was never sent, `obs` is not consulted.
pub fn decide(row: &RowFacts<'_>, obs: &Observation) -> Decision {
    if !row.sent {
        // Never sent, and once ended it can no longer be recorded, so it can never be sent.
        return if row.orphan {
            Decision::Refund(RefundReason::NeverSent)
        } else {
            WAIT
        };
    }
    let Observation::Seen {
        status,
        finalized_height,
        genesis,
    } = obs
    else {
        return WAIT;
    };
    match status {
        ChainStatus::Landed => Decision::Done,
        // Only an error at confirmed or finalized arrives as Failed (Ruling R7).
        ChainStatus::Failed(why) => Decision::Refund(RefundReason::FailedOnChain(why.clone())),
        ChainStatus::Processed => WAIT,
        ChainStatus::NotFound => {
            // Without both facts neither the reset nor the expiry can be proven.
            let (Some(last_valid), Some(signed_on)) =
                (row.last_valid_block_height, row.genesis_hash)
            else {
                return WAIT;
            };
            let Ok(last_valid) = u64::try_from(last_valid) else {
                return WAIT;
            };
            if genesis != signed_on {
                Decision::Refund(RefundReason::DevnetReset)
            } else if expired(*finalized_height, last_valid) {
                Decision::Refund(RefundReason::Expired)
            } else {
                Decision::Wait { resend: true }
            }
        },
    }
}

/// The blockhash is provably dead: the finalized height is past `last_valid` by more than
/// [`EXPIRY_MARGIN`]. A sum that does not fit is never expired.
fn expired(finalized_height: u64, last_valid: u64) -> bool {
    last_valid
        .checked_add(EXPIRY_MARGIN)
        .is_some_and(|dead_after| finalized_height > dead_after)
}

/// Read the chain for a sent row: the finalized height FIRST, then the status, then the genesis
/// (Ruling R9). Any error is [`Observation::Unreachable`].
pub async fn observe(chain: &dyn CoinChain, signature: &str) -> Observation {
    let seen = async {
        let finalized_height = chain.finalized_height().await?;
        let status = chain.status(signature).await?;
        let genesis = chain.genesis().await?;
        Ok::<_, ChainError>(Observation::Seen {
            status,
            finalized_height,
            genesis,
        })
    };
    match seen.await {
        Ok(obs) => obs,
        Err(e) => {
            tracing::debug!("crack-coin: {signature}: the chain is unreachable: {e}");
            Observation::Unreachable
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coin::fake::{Call, FakeChain};

    const GEN: &str = "genesis-a";
    const LAST: i64 = 1000;

    fn sent<'a>() -> RowFacts<'a> {
        RowFacts {
            sent: true,
            orphan: false,
            last_valid_block_height: Some(LAST),
            genesis_hash: Some(GEN),
        }
    }

    fn unsent<'a>(orphan: bool) -> RowFacts<'a> {
        RowFacts {
            sent: false,
            orphan,
            last_valid_block_height: None,
            genesis_hash: None,
        }
    }

    fn seen(status: ChainStatus, height: u64, genesis: &str) -> Observation {
        Observation::Seen {
            status,
            finalized_height: height,
            genesis: genesis.to_owned(),
        }
    }

    /// Every observation a row can meet, for rows whose decision must not depend on it.
    fn every_observation() -> Vec<Observation> {
        vec![
            Observation::Unreachable,
            seen(ChainStatus::Landed, 900, GEN),
            seen(ChainStatus::Failed("boom".into()), 900, GEN),
            seen(ChainStatus::Processed, 900, GEN),
            seen(ChainStatus::NotFound, 900, GEN),
            seen(ChainStatus::NotFound, 5000, GEN),
            seen(ChainStatus::NotFound, 900, "genesis-b"),
        ]
    }

    const WAIT: Decision = Decision::Wait { resend: false };
    const RESEND: Decision = Decision::Wait { resend: true };

    #[test]
    fn never_sent_orphan_refunds() {
        for obs in every_observation() {
            assert_eq!(
                decide(&unsent(true), &obs),
                Decision::Refund(RefundReason::NeverSent),
                "{obs:?}"
            );
        }
    }

    #[test]
    fn never_sent_fresh_waits() {
        for obs in every_observation() {
            assert_eq!(decide(&unsent(false), &obs), WAIT, "{obs:?}");
        }
    }

    #[test]
    fn landed_is_done() {
        for row in [
            sent(),
            RowFacts {
                orphan: true,
                ..sent()
            },
        ] {
            assert_eq!(
                decide(&row, &seen(ChainStatus::Landed, 900, GEN)),
                Decision::Done
            );
            // Landed is proof on its own: a later height or another genesis changes nothing.
            assert_eq!(
                decide(&row, &seen(ChainStatus::Landed, 5000, "genesis-b")),
                Decision::Done
            );
        }
    }

    #[test]
    fn failed_refunds_with_the_error() {
        assert_eq!(
            decide(
                &sent(),
                &seen(ChainStatus::Failed("custom 0x1".into()), 900, GEN)
            ),
            Decision::Refund(RefundReason::FailedOnChain("custom 0x1".into()))
        );
    }

    #[test]
    fn processed_waits() {
        // Even past expiry and on another genesis: processed is not proof either way.
        for obs in [
            seen(ChainStatus::Processed, 900, GEN),
            seen(ChainStatus::Processed, 5000, GEN),
            seen(ChainStatus::Processed, 900, "genesis-b"),
        ] {
            assert_eq!(decide(&sent(), &obs), WAIT, "{obs:?}");
        }
    }

    #[test]
    fn not_found_after_reset_refunds() {
        // The reset is checked before expiry: past expiry too, the reason is the reset.
        for height in [900, 5000] {
            assert_eq!(
                decide(&sent(), &seen(ChainStatus::NotFound, height, "genesis-b")),
                Decision::Refund(RefundReason::DevnetReset),
                "height {height}"
            );
        }
    }

    #[test]
    fn not_found_after_expiry_refunds() {
        assert_eq!(
            decide(&sent(), &seen(ChainStatus::NotFound, 5000, GEN)),
            Decision::Refund(RefundReason::Expired)
        );
    }

    #[test]
    fn not_found_before_expiry_waits_and_resends() {
        for height in [0, 900, 1000, 1001] {
            assert_eq!(
                decide(&sent(), &seen(ChainStatus::NotFound, height, GEN)),
                RESEND,
                "height {height}"
            );
        }
    }

    #[test]
    fn unreachable_waits() {
        // An orphan that was sent is still owed the chain's proof.
        for row in [
            sent(),
            RowFacts {
                orphan: true,
                ..sent()
            },
        ] {
            assert_eq!(decide(&row, &Observation::Unreachable), WAIT);
        }
    }

    #[test]
    fn expiry_needs_the_margin() {
        let at = |h: u64| decide(&sent(), &seen(ChainStatus::NotFound, h, GEN));
        assert_eq!(at(LAST as u64 + EXPIRY_MARGIN - 1), RESEND);
        assert_eq!(at(LAST as u64 + EXPIRY_MARGIN), RESEND);
        assert_eq!(
            at(LAST as u64 + EXPIRY_MARGIN + 1),
            Decision::Refund(RefundReason::Expired)
        );
        assert_eq!(EXPIRY_MARGIN, 150);
    }

    #[test]
    fn a_sent_row_missing_its_expiry_facts_waits() {
        let rows = [
            RowFacts {
                last_valid_block_height: None,
                ..sent()
            },
            RowFacts {
                genesis_hash: None,
                ..sent()
            },
            RowFacts {
                last_valid_block_height: None,
                genesis_hash: None,
                ..sent()
            },
            // A negative height is not a height.
            RowFacts {
                last_valid_block_height: Some(-1),
                ..sent()
            },
        ];
        for row in rows {
            for obs in [
                seen(ChainStatus::NotFound, 5000, GEN),
                seen(ChainStatus::NotFound, 5000, "genesis-b"),
                seen(ChainStatus::NotFound, 900, GEN),
            ] {
                assert_eq!(decide(&row, &obs), WAIT, "{row:?} {obs:?}");
            }
            // The chain's own proof still ends it.
            assert_eq!(
                decide(&row, &seen(ChainStatus::Landed, 900, GEN)),
                Decision::Done
            );
            assert_eq!(
                decide(&row, &seen(ChainStatus::Failed("e".into()), 900, GEN)),
                Decision::Refund(RefundReason::FailedOnChain("e".into()))
            );
        }
    }

    #[test]
    fn extreme_heights_neither_overflow_nor_refund_early() {
        let top = RowFacts {
            last_valid_block_height: Some(i64::MAX),
            ..sent()
        };
        assert_eq!(
            decide(
                &top,
                &seen(ChainStatus::NotFound, i64::MAX as u64 + 150, GEN)
            ),
            RESEND
        );
        assert_eq!(
            decide(&top, &seen(ChainStatus::NotFound, u64::MAX, GEN)),
            Decision::Refund(RefundReason::Expired)
        );
        let zero = RowFacts {
            last_valid_block_height: Some(0),
            ..sent()
        };
        assert_eq!(
            decide(&zero, &seen(ChainStatus::NotFound, 150, GEN)),
            RESEND
        );
        assert_eq!(
            decide(&zero, &seen(ChainStatus::NotFound, 151, GEN)),
            Decision::Refund(RefundReason::Expired)
        );
    }

    #[test]
    fn refund_reasons_have_fixed_db_text_and_parse_back() {
        let cases = [
            (RefundReason::NeverSent, "never sent"),
            (
                RefundReason::FailedOnChain("custom 0x1".into()),
                "failed on chain: custom 0x1",
            ),
            (RefundReason::DevnetReset, "devnet was reset"),
            (RefundReason::Expired, "expired"),
        ];
        for (reason, text) in cases {
            assert_eq!(reason.as_db_text(), text);
            assert_eq!(RefundReason::from_db_text(text), reason);
        }
        assert_eq!(
            RefundReason::from_db_text("a hand edit"),
            RefundReason::Other("a hand edit".into())
        );
        assert_eq!(
            RefundReason::Other("a hand edit".into()).as_db_text(),
            "a hand edit"
        );
    }

    #[tokio::test]
    async fn observe_reads_the_height_before_the_status() {
        let fake = FakeChain::new();
        fake.script_heights(vec![Ok(1234)]);
        fake.script_statuses(vec![Ok(ChainStatus::Processed)]);
        fake.set_genesis(Ok("g"));
        let obs = observe(&fake, "sig").await;
        assert_eq!(obs, seen(ChainStatus::Processed, 1234, "g"));
        assert_eq!(
            fake.calls(),
            vec![Call::Height, Call::Status("sig".into()), Call::Genesis]
        );
    }

    #[tokio::test]
    async fn any_chain_error_is_unreachable() {
        // Heights past expiry, so a half-read observation would look like proof.
        let height_down = FakeChain::new();
        height_down.script_heights(vec![Err("429 Too Many Requests")]);
        let status_down = FakeChain::new();
        status_down.script_heights(vec![Ok(5000)]);
        status_down.script_statuses(vec![Err("timed out")]);
        let genesis_down = FakeChain::new();
        genesis_down.script_heights(vec![Ok(5000)]);
        genesis_down.set_genesis(Err("connection reset"));
        for fake in [&height_down, &status_down, &genesis_down] {
            assert_eq!(observe(fake, "sig").await, Observation::Unreachable);
        }
        // A failed height read stops before the status is asked.
        assert_eq!(height_down.calls(), vec![Call::Height]);
    }
}
