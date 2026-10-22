//! A withdrawal's life (spec 6.1): [`begin`] charges, [`complete`] prepares, records, sends and
//! polls until the resolver decides. [`resolve_one`] is the sweeper's single step over the same
//! resolver, so the command and the sweeper can race on a row and still end it once.

use super::chain::{CoinChain, Signed};
use super::resolve::{decide, observe, Decision, Observation, RefundReason, RowFacts};
use crate::db::economy::coin::{
    finish_withdrawal, record_sent, refund_withdrawal, start_withdrawal, withdrawal_state,
    NewWithdrawal, PendingWithdrawal, Recorded, Recording, StartError, Started, WithdrawalState,
};
use serenity::all::{GuildId, UserId};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::Instant;

/// How often the command polls the chain.
pub const POLL_EVERY: Duration = Duration::from_secs(2);
/// How long the command polls before leaving the row to the sweeper.
pub const POLL_FOR: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub poll_every: Duration,
    pub poll_for: Duration,
}

impl Timing {
    pub const LIVE: Timing = Timing {
        poll_every: POLL_EVERY,
        poll_for: POLL_FOR,
    };
}

/// One `/wallet withdraw`. `address` is the member's link as read once, before charging: the row
/// snapshots it, and the mint goes there even if the member relinks meanwhile.
#[derive(Debug, Clone, Copy)]
pub struct WithdrawRequest<'a> {
    pub guild: GuildId,
    pub user: UserId,
    pub address: &'a str,
    pub amount: i64,
    /// This mint creates the member's token account (spec 2.4).
    pub sponsored: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Minted {
        signature: String,
    },
    /// `balance` is the member's balance after the refund, when this call made the refund. A
    /// refund made elsewhere (the sweeper) reports `None`.
    Refunded {
        reason: RefundReason,
        balance: Option<i64>,
    },
    /// Not settled within the window, or not known: the sweeper will end it.
    StillSettling,
}

/// Charge the member and write the `pending` row, snapshotting `req.address` and the chain's mint.
pub async fn begin(
    pool: &PgPool,
    chain: &dyn CoinChain,
    req: &WithdrawRequest<'_>,
) -> Result<Started, StartError> {
    start_withdrawal(
        pool,
        &NewWithdrawal {
            guild: req.guild,
            user: req.user,
            address: req.address,
            mint: chain.mint(),
            amount: req.amount,
        },
    )
    .await
}

/// Prepare, record, send and poll. Never sends unless `record_sent` moved the row; never ends a
/// row twice.
pub async fn complete(
    pool: &PgPool,
    chain: &dyn CoinChain,
    started: &Started,
    req: &WithdrawRequest<'_>,
    timing: &Timing,
) -> Outcome {
    let id = started.id;
    // 1. Prepare: sign, so the signature is known before anything is sent.
    let (signed, last_valid) = match prepare(chain, req).await {
        Ok(prepared) => prepared,
        Err(why) => {
            tracing::warn!("crack-coin: withdrawal {id}: not prepared, refunding: {why}");
            // This process's own row, never recorded, so never sent: a refund is safe now.
            let refund = Decision::Refund(RefundReason::NeverSent);
            return apply(pool, chain, id, None, refund)
                .await
                .unwrap_or(Outcome::StillSettling);
        },
    };
    // 2. Record. Only a row that moved here may be sent.
    let recording = Recording {
        signature: &signed.signature,
        tx: &signed.tx,
        last_valid_block_height: last_valid,
        genesis_hash: &signed.genesis,
        sponsored: req.sponsored,
    };
    let recorded = match record_sent(pool, id, &recording).await {
        Ok(Some(recorded)) => recorded,
        Ok(None) => {
            // Ended elsewhere (the sweeper refunded an orphan): send nothing.
            tracing::info!("crack-coin: withdrawal {id} ended before it was recorded; not sent");
            return ended_elsewhere(pool, id, None)
                .await
                .unwrap_or(Outcome::StillSettling);
        },
        Err(e) => {
            // The write may or may not have landed. Without a Recorded nothing may be sent; the
            // sweeper settles the row either way.
            tracing::warn!("crack-coin: withdrawal {id}: recording failed: {e}");
            return Outcome::StillSettling;
        },
    };
    // 3. Send. A send error is not a refund: only the chain's proof is.
    send(chain, &recorded).await;
    // 4. Poll until the resolver decides, or the window closes.
    let row = RowFacts {
        sent: true,
        orphan: false,
        last_valid_block_height: Some(last_valid),
        genesis_hash: Some(&signed.genesis),
    };
    let deadline = Instant::now() + timing.poll_for;
    while Instant::now() < deadline {
        tokio::time::sleep(timing.poll_every).await;
        let obs = observe(chain, recorded.signature()).await;
        if let Some(outcome) = apply(pool, chain, id, Some(&recorded), decide(&row, &obs)).await {
            return outcome;
        }
    }
    Outcome::StillSettling
}

/// Sign a mint of `req.amount` to `req.address`. Fails, so that the row is refunded unsent, when
/// the chain does or when a value cannot be stored.
async fn prepare(
    chain: &dyn CoinChain,
    req: &WithdrawRequest<'_>,
) -> Result<(Signed, i64), String> {
    let amount = u64::try_from(req.amount)
        .ok()
        .filter(|a| *a > 0)
        .ok_or_else(|| format!("the amount {} is not positive", req.amount))?;
    let signed = chain
        .prepare(req.address, amount)
        .await
        .map_err(|e| e.to_string())?;
    let last_valid = i64::try_from(signed.last_valid_block_height).map_err(|_| {
        format!(
            "the last valid block height {} does not fit",
            signed.last_valid_block_height
        )
    })?;
    Ok((signed, last_valid))
}

/// Send the recorded bytes, logging (never acting on) a failure.
async fn send(chain: &dyn CoinChain, recorded: &Recorded) {
    if let Err(e) = chain.send(recorded).await {
        tracing::warn!(
            "crack-coin: withdrawal {}: sending {} failed: {e}",
            recorded.id(),
            recorded.signature()
        );
    }
}

/// Carry out a decision. `Some` once the row has ended, here or elsewhere; `None` while it waits,
/// including after a database error (the next look retries).
async fn apply(
    pool: &PgPool,
    chain: &dyn CoinChain,
    id: i64,
    sent: Option<&Recorded>,
    decision: Decision,
) -> Option<Outcome> {
    match decision {
        Decision::Done => {
            // `decide` says Done only for a sent row.
            let signature = sent?.signature();
            match finish_withdrawal(pool, id).await {
                Ok(true) => Some(Outcome::Minted {
                    signature: signature.to_owned(),
                }),
                Ok(false) => ended_elsewhere(pool, id, Some(signature)).await,
                Err(e) => {
                    tracing::warn!("crack-coin: withdrawal {id}: marking it done failed: {e}");
                    None
                },
            }
        },
        Decision::Refund(reason) => match refund_withdrawal(pool, id, &reason.as_db_text()).await {
            Ok(Some(balance)) => Some(Outcome::Refunded {
                reason,
                balance: Some(balance),
            }),
            Ok(None) => ended_elsewhere(pool, id, sent.map(Recorded::signature)).await,
            Err(e) => {
                tracing::warn!("crack-coin: withdrawal {id}: refunding failed: {e}");
                None
            },
        },
        Decision::Wait { resend } => {
            if let (true, Some(recorded)) = (resend, sent) {
                send(chain, recorded).await;
            }
            None
        },
    }
}

/// Report a row that another resolver ended. `signature` is the one this caller recorded or
/// read; a `done` row is reported with it. `None` when the row is still pending, or unknown.
async fn ended_elsewhere(pool: &PgPool, id: i64, signature: Option<&str>) -> Option<Outcome> {
    match withdrawal_state(pool, id).await {
        Ok(Some((WithdrawalState::Done, _))) => signature.map(|s| Outcome::Minted {
            signature: s.to_owned(),
        }),
        Ok(Some((WithdrawalState::Refunded, reason))) => Some(Outcome::Refunded {
            reason: RefundReason::from_db_text(reason.as_deref().unwrap_or_default()),
            balance: None,
        }),
        Ok(Some((WithdrawalState::Pending, _))) => None,
        Ok(None) => {
            tracing::warn!("crack-coin: withdrawal {id} is gone");
            None
        },
        Err(e) => {
            tracing::warn!("crack-coin: withdrawal {id}: reading its state failed: {e}");
            None
        },
    }
}

/// The sweeper's single step: decide the row, end it if the chain proves how, and resend while
/// it may still land. `None` while it waits.
pub async fn resolve_one(
    pool: &PgPool,
    chain: &dyn CoinChain,
    row: &PendingWithdrawal,
) -> Option<Outcome> {
    let facts = RowFacts {
        sent: row.sent.is_some(),
        orphan: row.orphan,
        last_valid_block_height: row.last_valid_block_height,
        genesis_hash: row.genesis_hash.as_deref(),
    };
    let obs = match &row.sent {
        Some(recorded) => observe(chain, recorded.signature()).await,
        // Never sent: the decision does not look at the chain, so the chain is not asked.
        None => Observation::Unreachable,
    };
    apply(pool, chain, row.id, row.sent.as_ref(), decide(&facts, &obs)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coin::chain::ChainStatus;
    use crate::coin::fake::{self, FakeChain, Gate, GENESIS, LAST_VALID, SIG, TX};
    use crate::coin::sweep::sweep_once;
    use crate::db::economy::coin::{
        finish_withdrawal, link_address, pending_withdrawal, record_sent, refund_withdrawal,
        withdrawal_state, Recording, WithdrawalState,
    };
    use crate::db::economy::{balance, credit_earnings, EarnRow};
    use chrono::{Duration as ChronoDuration, Utc};

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(10);
    const ADDR: &str = "AddrA111111111111111111111111111111111111111";
    const ADDR_B: &str = "AddrB111111111111111111111111111111111111111";

    /// For outcomes the chain decides within a few polls. A test that reaches the end of this
    /// window has already failed.
    const QUICK: Timing = Timing {
        poll_every: Duration::from_millis(1),
        poll_for: Duration::from_secs(10),
    };
    /// For outcomes that must outlast the window.
    const SHORT: Timing = Timing {
        poll_every: Duration::from_millis(1),
        poll_for: Duration::from_millis(50),
    };

    async fn fund(pool: &PgPool, amount: i64) {
        credit_earnings(
            pool,
            G,
            &[EarnRow {
                user: U,
                voice: amount,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    fn req(address: &str) -> WithdrawRequest<'_> {
        WithdrawRequest {
            guild: G,
            user: U,
            address,
            amount: 30,
            sponsored: false,
        }
    }

    /// Fund 100, then withdraw 30 to [`ADDR`].
    async fn begun(pool: &PgPool, chain: &FakeChain) -> Started {
        fund(pool, 100).await;
        let s = begin(pool, chain, &req(ADDR)).await.unwrap();
        assert_eq!(s.balance, 70);
        s
    }

    async fn state(pool: &PgPool, id: i64) -> (WithdrawalState, Option<String>) {
        withdrawal_state(pool, id).await.unwrap().expect("the row")
    }

    async fn bal(pool: &PgPool) -> i64 {
        balance(pool, G, U).await.unwrap()
    }

    /// How many `coin_refund` ledger rows point at the withdrawal.
    async fn refunds(pool: &PgPool, id: i64) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM economy_ledger WHERE kind = 'coin_refund' AND coin_withdrawal_id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    type Stored = (
        String,
        String,
        Option<String>,
        Option<Vec<u8>>,
        Option<i64>,
        Option<String>,
        bool,
    );

    /// address, mint, signature, tx, last_valid_block_height, genesis_hash, sponsored
    async fn stored(pool: &PgPool, id: i64) -> Stored {
        sqlx::query_as(
            "SELECT address, mint, signature, tx, last_valid_block_height, genesis_hash, sponsored
             FROM coin_withdrawals WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// Wait for the gated poll. A flow that ends before reaching it fails here instead of hanging.
    async fn reached(gate: &Gate) {
        tokio::time::timeout(Duration::from_secs(20), gate.reached.notified())
            .await
            .expect("the flow never reached the gated poll");
    }

    fn minted() -> Outcome {
        Outcome::Minted {
            signature: SIG.to_owned(),
        }
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn lands_on_the_first_poll(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![Ok(ChainStatus::Landed)]);
        fund(&pool, 100).await;
        let r = WithdrawRequest {
            sponsored: true,
            ..req(ADDR)
        };
        let s = begin(&pool, &fake, &r).await.unwrap();
        assert_eq!(s.balance, 70);
        assert_eq!(complete(&pool, &fake, &s, &r, &QUICK).await, minted());
        assert_eq!(fake.prepares(), vec![(ADDR.to_owned(), 30)]);
        assert_eq!(fake.sends(), vec![TX.to_vec()]);
        assert_eq!(bal(&pool).await, 70);
        assert_eq!(state(&pool, s.id).await, (WithdrawalState::Done, None));
        assert_eq!(
            stored(&pool, s.id).await,
            (
                ADDR.to_owned(),
                fake::MINT.to_owned(),
                Some(SIG.to_owned()),
                Some(TX.to_vec()),
                Some(LAST_VALID as i64),
                Some(GENESIS.to_owned()),
                true
            ),
            "the address and mint snapshotted, then the transaction recorded before the send"
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn fails_on_chain_refunds(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![
            Ok(ChainStatus::Processed),
            Ok(ChainStatus::Failed("custom program error: 0x1".into())),
        ]);
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::FailedOnChain("custom program error: 0x1".into()),
                balance: Some(100),
            }
        );
        assert_eq!(fake.sends(), vec![TX.to_vec()], "no resend while processed");
        assert_eq!(bal(&pool).await, 100);
        assert_eq!(
            state(&pool, s.id).await,
            (
                WithdrawalState::Refunded,
                Some("failed on chain: custom program error: 0x1".into())
            )
        );
        assert_eq!(refunds(&pool, s.id).await, 1);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn never_seen_refunds_only_after_expiry(pool: PgPool) {
        let fake = FakeChain::new();
        // last_valid is 1000: 1150 is inside the margin, 1151 is past it.
        fake.script_heights(vec![Ok(900), Ok(1100), Ok(1150), Ok(1151)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound)]);
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::Expired,
                balance: Some(100),
            }
        );
        // The send, then a resend on each of the three polls that waited.
        assert_eq!(fake.sends(), vec![TX.to_vec(); 4]);
        assert_eq!(bal(&pool).await, 100);
        assert_eq!(
            state(&pool, s.id).await,
            (WithdrawalState::Refunded, Some("expired".into()))
        );
        assert_eq!(refunds(&pool, s.id).await, 1);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn unreachable_mid_settle_never_refunds(pool: PgPool) {
        let fake = FakeChain::new();
        // Far past expiry: if a rate limit read as "not found", this would refund.
        fake.script_heights(vec![Ok(50_000)]);
        fake.script_statuses(vec![Err("http error: 429 Too Many Requests")]);
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &SHORT).await,
            Outcome::StillSettling
        );
        assert_eq!(
            fake.sends(),
            vec![TX.to_vec()],
            "no resend while unreachable"
        );
        assert_eq!(bal(&pool).await, 70);
        assert_eq!(state(&pool, s.id).await, (WithdrawalState::Pending, None));
        assert_eq!(refunds(&pool, s.id).await, 0);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn unreachable_then_landed_is_done(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![
            Err("http error: timed out"),
            Err("http error: 429 Too Many Requests"),
            Ok(ChainStatus::Landed),
        ]);
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            minted()
        );
        assert_eq!(fake.sends(), vec![TX.to_vec()]);
        assert_eq!(bal(&pool).await, 70);
        assert_eq!(state(&pool, s.id).await, (WithdrawalState::Done, None));
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn devnet_reset_refunds(pool: PgPool) {
        let fake = FakeChain::new();
        let s = begun(&pool, &fake).await;
        // The transaction was signed on GENESIS; the chain now answers from another one, at a
        // height that has not expired anything.
        fake.set_genesis(Ok("fake-genesis-b"));
        fake.script_heights(vec![Ok(900)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound)]);
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::DevnetReset,
                balance: Some(100),
            }
        );
        assert_eq!(fake.sends(), vec![TX.to_vec()]);
        assert_eq!(bal(&pool).await, 100);
        assert_eq!(
            state(&pool, s.id).await,
            (WithdrawalState::Refunded, Some("devnet was reset".into()))
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_send_error_waits_for_the_chain(pool: PgPool) {
        // Even a preflight rejection is not proof: the same bytes may have been forwarded, so only
        // expiry may refund (spec 6.2).
        let fake = FakeChain::new();
        fake.set_send(Err("rpc error -32002: Transaction simulation failed"));
        fake.script_heights(vec![Ok(900), Ok(1100), Ok(1151)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound)]);
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::Expired,
                balance: Some(100),
            }
        );
        // The send, and a resend on each of the two polls that waited.
        assert_eq!(fake.sends(), vec![TX.to_vec(); 3]);
        assert_eq!(bal(&pool).await, 100);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn prepare_failure_refunds_never_sent(pool: PgPool) {
        let fake = FakeChain::new();
        fake.set_prepare(Err("rpc error -32002: blockhash not found"));
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::NeverSent,
                balance: Some(100),
            }
        );
        assert!(fake.sends().is_empty());
        assert_eq!(bal(&pool).await, 100);
        assert_eq!(
            state(&pool, s.id).await,
            (WithdrawalState::Refunded, Some("never sent".into()))
        );
        assert_eq!(stored(&pool, s.id).await.2, None, "nothing was recorded");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn values_that_cannot_be_stored_refund_never_sent(pool: PgPool) {
        // A last-valid height past i64::MAX can't be recorded, so it is never sent.
        let fake = FakeChain::new();
        fake.set_prepare(Ok(fake::signed(SIG, TX, u64::MAX, GENESIS)));
        let s = begun(&pool, &fake).await;
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::NeverSent,
                balance: Some(100),
            }
        );
        // An amount that is not a positive u64 is never even prepared.
        let s = begin(&pool, &fake, &req(ADDR)).await.unwrap();
        let bad = WithdrawRequest {
            amount: -1,
            ..req(ADDR)
        };
        assert_eq!(
            complete(&pool, &fake, &s, &bad, &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::NeverSent,
                balance: Some(100),
            }
        );
        assert_eq!(fake.prepares().len(), 1);
        assert!(fake.sends().is_empty());
        assert_eq!(bal(&pool).await, 100);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_row_refunded_by_the_sweeper_is_never_sent(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![Ok(ChainStatus::Landed)]);
        let s = begun(&pool, &fake).await;
        // A process start in the future makes the unsent row an orphan.
        let later = Some(Utc::now() + ChronoDuration::hours(1));
        assert_eq!(sweep_once(&pool, &fake, later).await, 1);
        assert_eq!(bal(&pool).await, 100);
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            Outcome::Refunded {
                reason: RefundReason::NeverSent,
                balance: None,
            }
        );
        assert!(fake.sends().is_empty(), "{:?}", fake.calls());
        assert_eq!(bal(&pool).await, 100, "credited once");
        assert_eq!(refunds(&pool, s.id).await, 1);
        assert_eq!(stored(&pool, s.id).await.2, None, "nothing was recorded");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn crash_after_record_is_settled_by_the_sweeper(pool: PgPool) {
        let fake = FakeChain::new();
        let s = begun(&pool, &fake).await;
        // The process recorded, then died before sending.
        record_sent(
            &pool,
            s.id,
            &Recording {
                signature: SIG,
                tx: TX,
                last_valid_block_height: LAST_VALID as i64,
                genesis_hash: GENESIS,
                sponsored: false,
            },
        )
        .await
        .unwrap()
        .expect("recorded");
        fake.script_heights(vec![Ok(900)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound), Ok(ChainStatus::Landed)]);
        let row = pending_withdrawal(&pool, s.id, None)
            .await
            .unwrap()
            .expect("pending");
        assert_eq!(resolve_one(&pool, &fake, &row).await, None);
        assert_eq!(fake.sends(), vec![TX.to_vec()], "the stored bytes, resent");
        assert_eq!(resolve_one(&pool, &fake, &row).await, Some(minted()));
        assert_eq!(fake.sends().len(), 1, "no resend once landed");
        assert_eq!(bal(&pool).await, 70);
        assert_eq!(state(&pool, s.id).await, (WithdrawalState::Done, None));
        assert!(fake.prepares().is_empty(), "the sweeper never signs");
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn relink_while_pending_mints_to_the_snapshot(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![Ok(ChainStatus::Landed)]);
        link_address(&pool, U, ADDR).await.unwrap();
        let s = begun(&pool, &fake).await;
        link_address(&pool, U, ADDR_B).await.unwrap();
        assert_eq!(
            complete(&pool, &fake, &s, &req(ADDR), &QUICK).await,
            minted()
        );
        assert_eq!(fake.prepares(), vec![(ADDR.to_owned(), 30)]);
        assert_eq!(stored(&pool, s.id).await.0, ADDR);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_refund_racing_the_poll_credits_once(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_heights(vec![Ok(900), Ok(900), Ok(5000)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound)]);
        let gate = fake.gate_status_call(3);
        let s = begun(&pool, &fake).await;
        let r = req(ADDR);
        let (out, ()) = tokio::join!(complete(&pool, &fake, &s, &r, &QUICK), async {
            // The sweeper proves expiry first, while the command's third poll is in flight.
            reached(&gate).await;
            assert_eq!(
                refund_withdrawal(&pool, s.id, "expired").await.unwrap(),
                Some(100)
            );
            gate.release.notify_one();
        });
        assert_eq!(
            out,
            Outcome::Refunded {
                reason: RefundReason::Expired,
                balance: None,
            }
        );
        assert_eq!(bal(&pool).await, 100, "credited once");
        assert_eq!(refunds(&pool, s.id).await, 1);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_row_finished_elsewhere_reports_the_mint(pool: PgPool) {
        let fake = FakeChain::new();
        fake.script_statuses(vec![Ok(ChainStatus::Landed)]);
        let gate = fake.gate_status_call(1);
        let s = begun(&pool, &fake).await;
        let r = req(ADDR);
        let (out, ()) = tokio::join!(complete(&pool, &fake, &s, &r, &QUICK), async {
            reached(&gate).await;
            assert!(finish_withdrawal(&pool, s.id).await.unwrap());
            gate.release.notify_one();
        });
        assert_eq!(out, minted());
        assert_eq!(bal(&pool).await, 70);
        assert_eq!(state(&pool, s.id).await, (WithdrawalState::Done, None));
    }
}
