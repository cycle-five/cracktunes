//! The coin sweeper (spec 6.4): every [`SWEEP_EVERY`], the resolver over every pending
//! withdrawal in every server. It covers a crash, a command whose poll died, and devnet being down
//! for a while. Nothing it does needs Discord.

use super::chain::CoinChain;
use super::flow::{resolve_row, Outcome, Step};
use crate::db::economy::coin::pending_withdrawals;
use crate::effects::spawn_supervised;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

pub const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// Spawn the supervised sweeper. `process_start` is the DB-clock start of this process: an unsent
/// row older than it is an orphan.
pub fn start_sweeper(
    pool: PgPool,
    chain: Arc<dyn CoinChain>,
    process_start: Option<DateTime<Utc>>,
) {
    spawn_supervised("coin-sweeper", async move {
        let mut tick = tokio::time::interval(SWEEP_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            sweep_once(&pool, chain.as_ref(), process_start).await;
        }
    });
}

/// What one sweep did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sweep {
    /// Rows it ended (minted or refunded).
    pub ended: usize,
    /// Signed rows it could not read the chain for.
    pub unreachable: usize,
}

/// One sweep: resolve every pending row. Warns once, with a count, when the chain was unreachable
/// for any of them.
pub async fn sweep_once(
    pool: &PgPool,
    chain: &dyn CoinChain,
    process_start: Option<DateTime<Utc>>,
) -> Sweep {
    let mut sweep = Sweep::default();
    let rows = match pending_withdrawals(pool, process_start).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("crack-coin: sweeper: reading pending withdrawals failed: {e}");
            return sweep;
        },
    };
    for row in &rows {
        match resolve_row(pool, chain, row).await {
            Step::Ended(Outcome::Minted { signature }) => {
                tracing::info!(
                    "crack-coin: sweeper: withdrawal {} of {} minted ({signature})",
                    row.id,
                    row.amount
                );
                sweep.ended += 1;
            },
            Step::Ended(Outcome::Refunded { reason, .. }) => {
                tracing::info!(
                    "crack-coin: sweeper: withdrawal {} of {} refunded: {}",
                    row.id,
                    row.amount,
                    reason.as_db_text()
                );
                sweep.ended += 1;
            },
            Step::Ended(Outcome::StillSettling) => {},
            Step::Waiting { unreachable } => {
                if unreachable {
                    sweep.unreachable += 1;
                }
            },
        }
    }
    if sweep.unreachable > 0 {
        tracing::warn!(
            "crack-coin: sweeper: the chain was unreachable for {} of {} pending withdrawals",
            sweep.unreachable,
            rows.len()
        );
    }
    sweep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coin::chain::ChainStatus;
    use crate::coin::fake::{FakeChain, GENESIS, LAST_VALID};
    use crate::db::economy::coin::{
        record_sent, start_withdrawal, withdrawal_state, NewWithdrawal, Recording, WithdrawalState,
    };
    use crate::db::economy::{balance, credit_earnings, EarnRow};
    use chrono::Duration as ChronoDuration;
    use serenity::all::{GuildId, UserId};

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

    const G: GuildId = GuildId::new(1);

    /// Fund `user` 100 and start a withdrawal of 30. With `sig`, record a transaction whose bytes
    /// are `[user]`.
    async fn row(pool: &PgPool, user: u64, sig: Option<&str>) -> i64 {
        let u = UserId::new(user);
        credit_earnings(
            pool,
            G,
            &[EarnRow {
                user: u,
                voice: 100,
                messages: 0,
            }],
        )
        .await
        .unwrap();
        let s = start_withdrawal(
            pool,
            &NewWithdrawal {
                guild: G,
                user: u,
                address: "Addr",
                mint: "Mint",
                amount: 30,
            },
        )
        .await
        .unwrap();
        if let Some(sig) = sig {
            record_sent(
                pool,
                s.id,
                &Recording {
                    signature: sig,
                    tx: &[user as u8],
                    last_valid_block_height: LAST_VALID as i64,
                    genesis_hash: GENESIS,
                    sponsored: false,
                },
            )
            .await
            .unwrap()
            .expect("recorded");
        }
        s.id
    }

    async fn ended(pool: &PgPool, id: i64) -> (WithdrawalState, Option<String>) {
        withdrawal_state(pool, id).await.unwrap().expect("the row")
    }

    async fn bal(pool: &PgPool, user: u64) -> i64 {
        balance(pool, G, UserId::new(user)).await.unwrap()
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_sweep_ends_what_the_chain_proves_and_waits_on_the_rest(pool: PgPool) {
        let landed = row(&pool, 1, Some("s1")).await;
        let failed = row(&pool, 2, Some("s2")).await;
        let fresh = row(&pool, 3, None).await;
        let unseen = row(&pool, 4, Some("s4")).await;
        let down = row(&pool, 5, Some("s5")).await;
        let old = row(&pool, 6, None).await;
        // Older than ORPHAN_AFTER_SECS, by the DB clock.
        sqlx::query(
            "UPDATE coin_withdrawals SET created_at = now() - interval '200 seconds' WHERE id = $1",
        )
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
        let fake = FakeChain::new();
        fake.script_heights(vec![Ok(900)]);
        fake.script_status_for("s1", vec![Ok(ChainStatus::Landed)]);
        fake.script_status_for("s2", vec![Ok(ChainStatus::Failed("e".into()))]);
        fake.script_status_for("s4", vec![Ok(ChainStatus::NotFound)]);
        fake.script_status_for("s5", vec![Err("http error: 429 Too Many Requests")]);
        // This process started before every row, so only age makes an orphan.
        let start = Some(Utc::now() - ChronoDuration::hours(1));
        assert_eq!(
            sweep_once(&pool, &fake, start).await,
            Sweep {
                ended: 3,
                unreachable: 1
            },
            "the 429 row is counted, once"
        );
        assert_eq!(ended(&pool, landed).await, (WithdrawalState::Done, None));
        assert_eq!(
            ended(&pool, failed).await,
            (WithdrawalState::Refunded, Some("failed on chain: e".into()))
        );
        assert_eq!(
            ended(&pool, old).await,
            (WithdrawalState::Refunded, Some("never sent".into()))
        );
        for waiting in [fresh, unseen, down] {
            assert_eq!(
                ended(&pool, waiting).await,
                (WithdrawalState::Pending, None)
            );
        }
        assert_eq!(
            fake.sends(),
            vec![vec![4u8]],
            "only the unseen one is resent"
        );
        let balances = [
            bal(&pool, 1).await,
            bal(&pool, 2).await,
            bal(&pool, 3).await,
            bal(&pool, 4).await,
            bal(&pool, 5).await,
            bal(&pool, 6).await,
        ];
        assert_eq!(balances, [70, 100, 70, 70, 70, 100]);
        assert!(fake.prepares().is_empty(), "the sweeper never signs");
        // The next sweep finds only the three still waiting, and ends none of them.
        assert_eq!(
            sweep_once(&pool, &fake, start).await,
            Sweep {
                ended: 0,
                unreachable: 1
            }
        );
        assert_eq!(bal(&pool, 2).await, 100, "a refund is never repeated");
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_crashed_row_never_seen_is_refunded_only_after_expiry(pool: PgPool) {
        // Recorded, then the process died before sending: each sweep resends the stored bytes
        // until the finalized height proves they can never land (last_valid 1000 + 150).
        let id = row(&pool, 1, Some("s1")).await;
        let fake = FakeChain::new();
        fake.script_heights(vec![Ok(1150), Ok(1151)]);
        fake.script_statuses(vec![Ok(ChainStatus::NotFound)]);
        let quiet = Sweep {
            ended: 0,
            unreachable: 0,
        };
        assert_eq!(sweep_once(&pool, &fake, None).await, quiet);
        assert_eq!(fake.sends(), vec![vec![1u8]]);
        assert_eq!(bal(&pool, 1).await, 70);
        assert_eq!(
            sweep_once(&pool, &fake, None).await,
            Sweep {
                ended: 1,
                unreachable: 0
            }
        );
        assert_eq!(
            ended(&pool, id).await,
            (WithdrawalState::Refunded, Some("expired".into()))
        );
        assert_eq!(bal(&pool, 1).await, 100);
        assert_eq!(fake.sends().len(), 1, "no resend once expired");
    }

    #[tokio::test]
    async fn a_sweep_without_a_database_ends_nothing_and_does_not_panic() {
        // Nothing listens on port 1, so every acquire waits out its timeout;
        // keep it short, as the outcome is the same at any length.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(100))
            .connect_lazy("postgresql://nobody:nothing@127.0.0.1:1/none")
            .unwrap();
        let fake = FakeChain::new();
        assert_eq!(sweep_once(&pool, &fake, None).await, Sweep::default());
        assert!(fake.calls().is_empty());
    }
}
