//! CrackCoin withdrawals: the money side. A withdrawal debits credits and
//! writes a `pending` row in one transaction, and ends exactly once, either
//! `done` (the coin was minted) or `refunded` (the credits came back). Every
//! transition is guarded by `WHERE state = 'pending'`, and a refund credits
//! only when its guarded `UPDATE` moved the row.

use super::{lock_wallet, set_balance, uid};
use crate::economy::wallet::{LedgerKind, MoveError};
use chrono::{DateTime, Utc};
use serenity::all::{GuildId, UserId};
use sqlx::PgPool;

/// A pending withdrawal with no signature is an orphan once it is this old: the
/// command that made it is surely gone.
pub const ORPHAN_AFTER_SECS: i64 = 120;

/// Where a withdrawal is in its life, stored as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawalState {
    Pending,
    Done,
    Refunded,
}

impl WithdrawalState {
    pub fn as_str(self) -> &'static str {
        match self {
            WithdrawalState::Pending => "pending",
            WithdrawalState::Done => "done",
            WithdrawalState::Refunded => "refunded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            WithdrawalState::Pending,
            WithdrawalState::Done,
            WithdrawalState::Refunded,
        ]
        .into_iter()
        .find(|st| st.as_str() == s)
    }
}

fn parse_state(s: &str) -> sqlx::Result<WithdrawalState> {
    WithdrawalState::parse(s)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown withdrawal state {s:?}").into()))
}

/// Like `write_ledger`, but the row points at a withdrawal instead of an action.
async fn write_coin_ledger(
    tx: &mut sqlx::PgConnection,
    guild: GuildId,
    user: UserId,
    delta: i64,
    kind: LedgerKind,
    withdrawal_id: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO economy_ledger (guild_id, user_id, delta, kind, coin_withdrawal_id)
         VALUES ($1, $2, $3, $4, $5)",
        guild.get() as i64,
        user.get() as i64,
        delta,
        kind.as_str(),
        withdrawal_id
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// A withdrawal to start. `address` and `mint` are snapshotted into the row.
pub struct NewWithdrawal<'a> {
    pub guild: GuildId,
    pub user: UserId,
    pub address: &'a str,
    pub mint: &'a str,
    pub amount: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    pub id: i64,
    /// What the member holds in that server after the debit.
    pub balance: i64,
}

#[derive(Debug)]
pub enum StartError {
    Money(MoveError),
    /// The member already has a withdrawal in flight (in any server).
    AlreadyPending,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for StartError {
    fn from(e: sqlx::Error) -> Self {
        StartError::Db(e)
    }
}

impl From<MoveError> for StartError {
    fn from(e: MoveError) -> Self {
        StartError::Money(e)
    }
}

/// Insert the `pending` row, then debit the member and write the
/// `coin_withdraw` ledger row, in one transaction. Nothing is written on any
/// refusal.
///
/// Lock order: the withdrawal row (the insert, which also waits on any other
/// resolver of this member's old pending row through the unique index), then
/// the wallet. [`refund_withdrawal`] and [`refund_unsent`] take the same
/// order; the reverse deadlocks.
pub async fn start_withdrawal(pool: &PgPool, w: &NewWithdrawal<'_>) -> Result<Started, StartError> {
    if w.amount <= 0 {
        return Err(StartError::Money(MoveError::Invalid));
    }
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        "INSERT INTO coin_withdrawals (guild_id, user_id, address, mint, amount)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
        w.guild.get() as i64,
        w.user.get() as i64,
        w.address,
        w.mint,
        w.amount
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d)
            if d.code().as_deref() == Some("23505")
                && d.constraint() == Some("coin_withdrawals_one_pending") =>
        {
            StartError::AlreadyPending
        },
        _ => StartError::Db(e),
    })?;
    let wallet = lock_wallet(&mut tx, w.guild, w.user).await?;
    if wallet.balance < w.amount {
        // Dropping the transaction rolls the row back.
        return Err(StartError::Money(MoveError::Insufficient {
            balance: wallet.balance,
        }));
    }
    let left = wallet.balance - w.amount;
    set_balance(&mut tx, &wallet, left).await?;
    write_coin_ledger(
        &mut tx,
        w.guild,
        w.user,
        -w.amount,
        LedgerKind::CoinWithdraw,
        row.id,
    )
    .await?;
    tx.commit().await?;
    Ok(Started {
        id: row.id,
        balance: left,
    })
}

/// What a prepared transaction stores before it may be sent.
pub struct Recording<'a> {
    pub signature: &'a str,
    pub tx: &'a [u8],
    pub last_valid_block_height: i64,
    pub genesis_hash: &'a str,
    /// This transaction creates the member's token account.
    pub sponsored: bool,
}

/// Proof that a transaction was stored before sending. Only [`record_sent`] and
/// the `pending_*` reads make one, and only for a row that holds a signature
/// and its bytes, so "send before recording" cannot be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    id: i64,
    signature: String,
    tx: Vec<u8>,
}

impl Recorded {
    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn tx(&self) -> &[u8] {
        &self.tx
    }
}

/// Store the signature and the signed bytes, only while the row is still
/// `pending` and has none. `None` means the row moved on (or already holds a
/// signature): do not send.
pub async fn record_sent(
    pool: &PgPool,
    id: i64,
    r: &Recording<'_>,
) -> sqlx::Result<Option<Recorded>> {
    let row = sqlx::query!(
        "UPDATE coin_withdrawals
         SET signature = $2, tx = $3, last_valid_block_height = $4,
             genesis_hash = $5, sponsored = $6
         WHERE id = $1 AND state = 'pending' AND signature IS NULL
         RETURNING id",
        id,
        r.signature,
        r.tx,
        r.last_valid_block_height,
        r.genesis_hash,
        r.sponsored
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| Recorded {
        id: row.id,
        signature: r.signature.to_owned(),
        tx: r.tx.to_vec(),
    }))
}

/// `pending` to `done`, only once a signature is recorded (a mint cannot be done without a
/// send). True when this call moved the row.
pub async fn finish_withdrawal(pool: &PgPool, id: i64) -> sqlx::Result<bool> {
    let moved = sqlx::query!(
        "UPDATE coin_withdrawals SET state = 'done', ended_at = now()
         WHERE id = $1 AND state = 'pending' AND signature IS NOT NULL",
        id
    )
    .execute(pool)
    .await?;
    Ok(moved.rows_affected() > 0)
}

/// `pending` to `refunded`, crediting the amount back, in one transaction, only for a row that
/// has a signature (an unsigned row is [`refund_unsent`]'s). The
/// credit happens only when the guarded `UPDATE` moved the row, so racing
/// resolvers credit once. `Some(balance)` only when this call did it.
///
/// Lock order: the withdrawal row, then the wallet (as [`start_withdrawal`]).
pub async fn refund_withdrawal(pool: &PgPool, id: i64, reason: &str) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "UPDATE coin_withdrawals
         SET state = 'refunded', reason = $2, ended_at = now()
         WHERE id = $1 AND state = 'pending' AND signature IS NOT NULL
         RETURNING guild_id, user_id, amount",
        id,
        reason
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let back = credit_back(&mut tx, id, row.guild_id, row.user_id, row.amount).await?;
    tx.commit().await?;
    Ok(Some(back))
}

/// The reason [`refund_unsent`] stores.
pub const NEVER_SENT_REASON: &str = "never sent";

/// Refund a withdrawal that was never recorded, so never sent: `pending` to
/// `refunded` with [`NEVER_SENT_REASON`], only while it still has no
/// signature. A row recorded since the caller looked moves nothing (`None`):
/// its transaction may be on the way, so only the chain may end it.
///
/// One transaction and one lock order, as [`refund_withdrawal`].
pub async fn refund_unsent(pool: &PgPool, id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "UPDATE coin_withdrawals
         SET state = 'refunded', reason = $2, ended_at = now()
         WHERE id = $1 AND state = 'pending' AND signature IS NULL
         RETURNING guild_id, user_id, amount",
        id,
        NEVER_SENT_REASON
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let back = credit_back(&mut tx, id, row.guild_id, row.user_id, row.amount).await?;
    tx.commit().await?;
    Ok(Some(back))
}

/// Credit a refunded withdrawal's amount back and write its `coin_refund`
/// ledger row, inside the transaction whose guarded `UPDATE` just moved the
/// row: the row is locked first, then the wallet. Returns the new balance.
async fn credit_back(
    tx: &mut sqlx::PgConnection,
    id: i64,
    guild_id: i64,
    user_id: i64,
    amount: i64,
) -> sqlx::Result<i64> {
    let (Some(guild), Some(user)) = (
        u64::try_from(guild_id)
            .ok()
            .filter(|g| *g != 0 && *g != u64::MAX),
        uid(user_id),
    ) else {
        return Err(sqlx::Error::Decode("withdrawal has an invalid id".into()));
    };
    let guild = GuildId::new(guild);
    let wallet = lock_wallet(tx, guild, user).await?;
    let back = wallet
        .balance
        .checked_add(amount)
        .ok_or_else(|| sqlx::Error::Decode("refund overflows the balance".into()))?;
    set_balance(tx, &wallet, back).await?;
    write_coin_ledger(tx, guild, user, amount, LedgerKind::CoinRefund, id).await?;
    Ok(back)
}

/// A withdrawal still in flight, as the resolver sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWithdrawal {
    pub id: i64,
    pub guild: GuildId,
    pub user: UserId,
    pub address: String,
    pub amount: i64,
    /// Set once the transaction was recorded: the row may have been sent, and
    /// only the chain may end it (bar the never-sent orphan rule).
    pub signature: Option<String>,
    /// The recorded transaction, for a resend. Present only when the row holds
    /// both its signature and its bytes.
    pub sent: Option<Recorded>,
    pub last_valid_block_height: Option<i64>,
    pub genesis_hash: Option<String>,
    /// Created before this process started, or older than [`ORPHAN_AFTER_SECS`],
    /// by the database clock.
    pub orphan: bool,
}

struct PendingRow {
    id: i64,
    guild_id: i64,
    user_id: i64,
    address: String,
    amount: i64,
    signature: Option<String>,
    tx: Option<Vec<u8>>,
    last_valid_block_height: Option<i64>,
    genesis_hash: Option<String>,
    orphan: bool,
}

fn pending_from(r: PendingRow) -> sqlx::Result<PendingWithdrawal> {
    let (Some(guild), Some(user)) = (
        u64::try_from(r.guild_id)
            .ok()
            .filter(|g| *g != 0 && *g != u64::MAX),
        uid(r.user_id),
    ) else {
        return Err(sqlx::Error::Decode("withdrawal has an invalid id".into()));
    };
    let sent = match (&r.signature, r.tx) {
        (Some(signature), Some(tx)) => Some(Recorded {
            id: r.id,
            signature: signature.clone(),
            tx,
        }),
        _ => None,
    };
    Ok(PendingWithdrawal {
        id: r.id,
        guild: GuildId::new(guild),
        user,
        address: r.address,
        amount: r.amount,
        signature: r.signature,
        sent,
        last_valid_block_height: r.last_valid_block_height,
        genesis_hash: r.genesis_hash,
        orphan: r.orphan,
    })
}

/// Every `pending` withdrawal, oldest first.
pub async fn pending_withdrawals(
    pool: &PgPool,
    process_start: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<PendingWithdrawal>> {
    let rows = sqlx::query_as!(
        PendingRow,
        r#"SELECT id, guild_id, user_id, address, amount, signature, tx,
                  last_valid_block_height, genesis_hash,
                  (COALESCE(created_at < $1::timestamptz, false)
                   OR created_at < now() - make_interval(secs => $2::float8)) AS "orphan!"
           FROM coin_withdrawals WHERE state = 'pending' ORDER BY id"#,
        process_start,
        ORPHAN_AFTER_SECS as f64
    )
    .fetch_all(pool)
    .await?;
    // One undecodable row (a hand edit) must not stall settlement for everyone: skip it.
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let id = row.id;
            match pending_from(row) {
                Ok(w) => Some(w),
                Err(err) => {
                    tracing::warn!("crack-coin: skipping undecodable withdrawal {id}: {err}");
                    None
                },
            }
        })
        .collect())
}

/// One withdrawal if it is still `pending`.
pub async fn pending_withdrawal(
    pool: &PgPool,
    id: i64,
    process_start: Option<DateTime<Utc>>,
) -> sqlx::Result<Option<PendingWithdrawal>> {
    let row = sqlx::query_as!(
        PendingRow,
        r#"SELECT id, guild_id, user_id, address, amount, signature, tx,
                  last_valid_block_height, genesis_hash,
                  (COALESCE(created_at < $2::timestamptz, false)
                   OR created_at < now() - make_interval(secs => $3::float8)) AS "orphan!"
           FROM coin_withdrawals WHERE id = $1 AND state = 'pending'"#,
        id,
        process_start,
        ORPHAN_AFTER_SECS as f64
    )
    .fetch_optional(pool)
    .await?;
    row.map(pending_from).transpose()
}

/// A withdrawal's state and, once refunded, why.
pub async fn withdrawal_state(
    pool: &PgPool,
    id: i64,
) -> sqlx::Result<Option<(WithdrawalState, Option<String>)>> {
    let row = sqlx::query!(
        "SELECT state, reason FROM coin_withdrawals WHERE id = $1",
        id
    )
    .fetch_optional(pool)
    .await?;
    row.map(|r| Ok((parse_state(&r.state)?, r.reason)))
        .transpose()
}

/// Link (or replace) the member's address; one per Discord user.
pub async fn link_address(pool: &PgPool, user: UserId, address: &str) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO coin_links (user_id, address) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET address = EXCLUDED.address, linked_at = now()",
        user.get() as i64,
        address
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// True when there was a link to remove.
pub async fn unlink_address(pool: &PgPool, user: UserId) -> sqlx::Result<bool> {
    let gone = sqlx::query!(
        "DELETE FROM coin_links WHERE user_id = $1",
        user.get() as i64
    )
    .execute(pool)
    .await?;
    Ok(gone.rows_affected() > 0)
}

pub async fn linked_address(pool: &PgPool, user: UserId) -> sqlx::Result<Option<String>> {
    let row = sqlx::query!(
        "SELECT address FROM coin_links WHERE user_id = $1",
        user.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.address))
}

/// How many token accounts we created for this member for this mint: `done`
/// rows that were sponsored, across all servers.
pub async fn sponsored_accounts(pool: &PgPool, user: UserId, mint: &str) -> sqlx::Result<i64> {
    let row = sqlx::query!(
        r#"SELECT count(*) AS "n!" FROM coin_withdrawals
           WHERE user_id = $1 AND mint = $2 AND state = 'done' AND sponsored"#,
        user.get() as i64,
        mint
    )
    .fetch_one(pool)
    .await?;
    Ok(row.n)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastWithdrawal {
    pub amount: i64,
    pub state: WithdrawalState,
    pub signature: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletOverview {
    /// The sum of the member's `done` withdrawals, across all servers.
    pub total_withdrawn: i64,
    pub last: Option<LastWithdrawal>,
}

pub async fn wallet_overview(pool: &PgPool, user: UserId) -> sqlx::Result<WalletOverview> {
    let total = sqlx::query!(
        r#"SELECT COALESCE(SUM(amount), 0)::bigint AS "total!" FROM coin_withdrawals
           WHERE user_id = $1 AND state = 'done'"#,
        user.get() as i64
    )
    .fetch_one(pool)
    .await?;
    let last = sqlx::query!(
        "SELECT amount, state, signature, reason, created_at FROM coin_withdrawals
         WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1",
        user.get() as i64
    )
    .fetch_optional(pool)
    .await?
    .map(|r| {
        Ok::<_, sqlx::Error>(LastWithdrawal {
            amount: r.amount,
            state: parse_state(&r.state)?,
            signature: r.signature,
            reason: r.reason,
            created_at: r.created_at,
        })
    })
    .transpose()?;
    Ok(WalletOverview {
        total_withdrawn: total.total,
        last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::economy::{balance, credit_earnings, EarnRow};
    use crate::economy::wallet::{LedgerKind, MoveError};
    use chrono::{Duration, Utc};
    use serenity::all::{GuildId, UserId};
    use sqlx::PgPool;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    const G: GuildId = GuildId::new(1);
    const G2: GuildId = GuildId::new(2);
    const U: UserId = UserId::new(10);
    const ADDR: &str = "Addr1111111111111111111111111111111111111111";
    const MINT: &str = "Mint1111111111111111111111111111111111111111";

    async fn fund(pool: &PgPool, guild: GuildId, user: UserId, amount: i64) {
        credit_earnings(
            pool,
            guild,
            &[EarnRow {
                user,
                voice: amount,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    fn new_w(guild: GuildId, user: UserId, amount: i64) -> NewWithdrawal<'static> {
        NewWithdrawal {
            guild,
            user,
            address: ADDR,
            mint: MINT,
            amount,
        }
    }

    fn rec<'a>(sig: &'a str, tx: &'a [u8], sponsored: bool) -> Recording<'a> {
        Recording {
            signature: sig,
            tx,
            last_valid_block_height: 500,
            genesis_hash: "genesis",
            sponsored,
        }
    }

    /// Fund 100, start a withdrawal of 30 in `G`.
    async fn started(pool: &PgPool) -> Started {
        fund(pool, G, U, 100).await;
        start_withdrawal(pool, &new_w(G, U, 30)).await.unwrap()
    }

    /// `started`, then its transaction recorded (so it has a signature).
    async fn signed(pool: &PgPool) -> Started {
        let s = started(pool).await;
        record_sent(pool, s.id, &rec("sig", &[1], false))
            .await
            .unwrap()
            .expect("recorded");
        s
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn start_debits_and_writes_a_pending_row_and_a_ledger_row(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        let s = started(&pool).await;
        assert_eq!(s.balance, 70);
        assert_eq!(balance(&pool, G, U).await?, 70);
        assert_eq!(
            withdrawal_state(&pool, s.id).await?,
            Some((WithdrawalState::Pending, None))
        );
        let row = sqlx::query!(
            "SELECT guild_id, user_id, address, mint, amount FROM coin_withdrawals WHERE id = $1",
            s.id
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(
            (
                row.guild_id,
                row.user_id,
                row.address.as_str(),
                row.mint.as_str(),
                row.amount
            ),
            (1, 10, ADDR, MINT, 30)
        );
        let l = sqlx::query!(
            "SELECT delta, kind, action_id, coin_withdrawal_id FROM economy_ledger
             WHERE guild_id = 1 AND user_id = 10"
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].delta, -30);
        assert_eq!(l[0].kind, LedgerKind::CoinWithdraw.as_str());
        assert_eq!(l[0].coin_withdrawal_id, Some(s.id));
        assert_eq!(l[0].action_id, None);
        Ok(())
    }

    async fn nothing_written(pool: &PgPool) -> sqlx::Result<()> {
        let w = sqlx::query!(r#"SELECT count(*) AS "n!" FROM coin_withdrawals"#)
            .fetch_one(pool)
            .await?;
        let l = sqlx::query!(r#"SELECT count(*) AS "n!" FROM economy_ledger"#)
            .fetch_one(pool)
            .await?;
        assert_eq!((w.n, l.n), (0, 0));
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn start_refuses_more_than_the_balance_and_writes_nothing(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        fund(&pool, G, U, 100).await;
        for amount in [101, i64::MAX] {
            match start_withdrawal(&pool, &new_w(G, U, amount)).await {
                Err(StartError::Money(MoveError::Insufficient { balance: 100 })) => {},
                other => panic!("expected Insufficient(100), got {other:?}"),
            }
        }
        assert_eq!(balance(&pool, G, U).await?, 100);
        nothing_written(&pool).await
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn start_refuses_a_non_positive_amount(pool: PgPool) -> sqlx::Result<()> {
        fund(&pool, G, U, 100).await;
        for amount in [0, -5, i64::MIN] {
            assert!(matches!(
                start_withdrawal(&pool, &new_w(G, U, amount)).await,
                Err(StartError::Money(MoveError::Invalid))
            ));
        }
        assert_eq!(balance(&pool, G, U).await?, 100);
        nothing_written(&pool).await
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_second_pending_withdrawal_is_refused_even_in_another_server(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        started(&pool).await;
        fund(&pool, G2, U, 50).await;
        assert!(matches!(
            start_withdrawal(&pool, &new_w(G2, U, 10)).await,
            Err(StartError::AlreadyPending)
        ));
        assert_eq!(balance(&pool, G2, U).await?, 50, "the debit rolled back");
        assert_eq!(balance(&pool, G, U).await?, 70);
        let n = sqlx::query!(r#"SELECT count(*) AS "n!" FROM economy_ledger WHERE guild_id = 2"#)
            .fetch_one(&pool)
            .await?;
        assert_eq!(n.n, 0);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn start_refuses_before_it_touches_the_wallet(pool: PgPool) -> sqlx::Result<()> {
        // The withdrawal row comes first in the lock order. Hold the wallet
        // locked on another connection: a second start must say
        // AlreadyPending at once, not wait for the wallet. (With the wallet
        // first it would wait here, and the timeout turns the hang into a
        // failure.)
        started(&pool).await;
        let mut holder = pool.begin().await?;
        sqlx::query!(
            "SELECT balance FROM economy_wallets WHERE guild_id = 1 AND user_id = 10 FOR UPDATE"
        )
        .fetch_one(&mut *holder)
        .await?;
        let got = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            start_withdrawal(&pool, &new_w(G, U, 10)),
        )
        .await
        .expect("start blocked on the wallet lock");
        assert!(matches!(got, Err(StartError::AlreadyPending)), "{got:?}");
        holder.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn done_then_refund_moves_nothing(pool: PgPool) -> sqlx::Result<()> {
        let s = signed(&pool).await;
        assert!(finish_withdrawal(&pool, s.id).await?);
        assert_eq!(refund_withdrawal(&pool, s.id, "late").await?, None);
        assert_eq!(balance(&pool, G, U).await?, 70);
        assert!(!finish_withdrawal(&pool, s.id).await?, "done is final");
        assert_eq!(
            withdrawal_state(&pool, s.id).await?,
            Some((WithdrawalState::Done, None))
        );
        let n = sqlx::query!(
            r#"SELECT count(*) AS "n!" FROM economy_ledger WHERE kind = 'coin_refund'"#
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(n.n, 0);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn refund_restores_the_balance_once(pool: PgPool) -> sqlx::Result<()> {
        let s = signed(&pool).await;
        assert_eq!(refund_withdrawal(&pool, s.id, "expired").await?, Some(100));
        assert_eq!(refund_withdrawal(&pool, s.id, "again").await?, None);
        assert_eq!(balance(&pool, G, U).await?, 100);
        let l = sqlx::query!(
            "SELECT delta, coin_withdrawal_id FROM economy_ledger WHERE kind = 'coin_refund'"
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].delta, l[0].coin_withdrawal_id), (30, Some(s.id)));
        assert_eq!(
            withdrawal_state(&pool, s.id).await?,
            Some((WithdrawalState::Refunded, Some("expired".to_string())))
        );
        assert!(!finish_withdrawal(&pool, s.id).await?, "refunded is final");
        // The member may withdraw again.
        assert!(start_withdrawal(&pool, &new_w(G, U, 10)).await.is_ok());
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn racing_refunds_credit_once(pool: PgPool) -> sqlx::Result<()> {
        let s = signed(&pool).await;
        let (a, b) = tokio::join!(
            refund_withdrawal(&pool, s.id, "a"),
            refund_withdrawal(&pool, s.id, "b")
        );
        let got = [a?, b?];
        assert_eq!(got.iter().filter(|r| r.is_some()).count(), 1, "{got:?}");
        assert_eq!(balance(&pool, G, U).await?, 100);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn record_on_a_refunded_row_returns_none(pool: PgPool) -> sqlx::Result<()> {
        let s = started(&pool).await;
        refund_unsent(&pool, s.id).await?;
        assert!(record_sent(&pool, s.id, &rec("sig", &[1, 2], false))
            .await?
            .is_none());
        let r = sqlx::query!(
            "SELECT signature, tx FROM coin_withdrawals WHERE id = $1",
            s.id
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!((r.signature, r.tx), (None, None));
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_signed_guards_move_nothing_on_an_unsigned_row(pool: PgPool) -> sqlx::Result<()> {
        let s = started(&pool).await;
        assert_eq!(refund_withdrawal(&pool, s.id, "expired").await?, None);
        assert!(!finish_withdrawal(&pool, s.id).await?);
        assert_eq!(balance(&pool, G, U).await?, 70, "nothing credited");
        assert_eq!(refunds_of(&pool, s.id).await?, 0);
        assert_eq!(
            withdrawal_state(&pool, s.id).await?,
            Some((WithdrawalState::Pending, None))
        );
        // The partition: the unsigned refund still works.
        assert_eq!(refund_unsent(&pool, s.id).await?, Some(100));
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn one_undecodable_pending_row_does_not_hide_the_rest(pool: PgPool) -> sqlx::Result<()> {
        let good = started(&pool).await;
        // A hand edit: guild 0 is not a valid server id.
        fund(&pool, G2, UserId::new(11), 50).await;
        let bad = start_withdrawal(&pool, &new_w(G2, UserId::new(11), 10))
            .await
            .unwrap();
        sqlx::query!(
            "UPDATE coin_withdrawals SET guild_id = 0 WHERE id = $1",
            bad.id
        )
        .execute(&pool)
        .await?;
        let listed = pending_withdrawals(&pool, None).await?;
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].id, good.id);
        Ok(())
    }

    /// How many `coin_refund` ledger rows point at the withdrawal.
    async fn refunds_of(pool: &PgPool, id: i64) -> sqlx::Result<i64> {
        sqlx::query_scalar(
            "SELECT count(*) FROM economy_ledger WHERE kind = 'coin_refund' AND coin_withdrawal_id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn refund_unsent_refunds_only_an_unsigned_pending_row(pool: PgPool) -> sqlx::Result<()> {
        // Unsigned and pending: refunded once, as never sent.
        let a = started(&pool).await;
        assert_eq!(refund_unsent(&pool, a.id).await?, Some(100));
        assert_eq!(balance(&pool, G, U).await?, 100);
        assert_eq!(
            withdrawal_state(&pool, a.id).await?,
            Some((
                WithdrawalState::Refunded,
                Some(NEVER_SENT_REASON.to_string())
            ))
        );
        assert_eq!(refunds_of(&pool, a.id).await?, 1);
        // Already refunded: nothing moves.
        assert_eq!(refund_unsent(&pool, a.id).await?, None);
        assert_eq!(balance(&pool, G, U).await?, 100);
        assert_eq!(refunds_of(&pool, a.id).await?, 1);
        // Signed: its transaction may be on the way, so nothing moves.
        let b = start_withdrawal(&pool, &new_w(G, U, 30)).await.unwrap();
        record_sent(&pool, b.id, &rec("sigB", &[1], false))
            .await?
            .expect("recorded");
        assert_eq!(refund_unsent(&pool, b.id).await?, None);
        assert_eq!(balance(&pool, G, U).await?, 70);
        assert_eq!(
            withdrawal_state(&pool, b.id).await?,
            Some((WithdrawalState::Pending, None))
        );
        assert_eq!(refunds_of(&pool, b.id).await?, 0);
        // Done: nothing moves.
        assert!(finish_withdrawal(&pool, b.id).await?);
        assert_eq!(refund_unsent(&pool, b.id).await?, None);
        assert_eq!(balance(&pool, G, U).await?, 70);
        assert_eq!(
            withdrawal_state(&pool, b.id).await?,
            Some((WithdrawalState::Done, None))
        );
        assert_eq!(refunds_of(&pool, b.id).await?, 0);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_signature_without_its_bytes_still_reads_as_signed(pool: PgPool) -> sqlx::Result<()> {
        let s = started(&pool).await;
        record_sent(&pool, s.id, &rec("sigA", &[1, 2], false))
            .await?
            .expect("recorded");
        // A hand edit: our code never stores a signature without its bytes.
        sqlx::query("UPDATE coin_withdrawals SET tx = NULL WHERE id = $1")
            .bind(s.id)
            .execute(&pool)
            .await?;
        let p = pending_withdrawal(&pool, s.id, None)
            .await?
            .expect("pending");
        assert_eq!(p.signature.as_deref(), Some("sigA"));
        assert_eq!(p.sent, None, "no bytes, so nothing to resend");
        let listed = pending_withdrawals(&pool, None).await?;
        assert_eq!(listed[0].signature.as_deref(), Some("sigA"));
        assert_eq!(listed[0].sent, None);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn record_sets_signature_tx_and_sponsored(pool: PgPool) -> sqlx::Result<()> {
        let s = started(&pool).await;
        let tx = [9u8, 8, 7, 0, 255];
        let got = record_sent(&pool, s.id, &rec("sigA", &tx, true))
            .await?
            .expect("recorded");
        assert_eq!(
            (got.id(), got.signature(), got.tx()),
            (s.id, "sigA", &tx[..])
        );
        let r = sqlx::query!(
            "SELECT signature, last_valid_block_height, genesis_hash, sponsored
             FROM coin_withdrawals WHERE id = $1",
            s.id
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(r.signature.as_deref(), Some("sigA"));
        assert_eq!(r.last_valid_block_height, Some(500));
        assert_eq!(r.genesis_hash.as_deref(), Some("genesis"));
        assert!(r.sponsored);
        // A second record never overwrites the first: the signature is final.
        assert!(record_sent(&pool, s.id, &rec("sigB", &[1], false))
            .await?
            .is_none());
        let p = pending_withdrawal(&pool, s.id, None)
            .await?
            .expect("pending");
        assert_eq!(p.sent.as_ref().map(|r| r.signature()), Some("sigA"));
        assert_eq!(p.signature.as_deref(), Some("sigA"));
        assert_eq!(p.last_valid_block_height, Some(500));
        assert_eq!(p.genesis_hash.as_deref(), Some("genesis"));
        assert_eq!((p.guild, p.user, p.amount), (G, U, 30));
        assert_eq!(p.address, ADDR);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn pending_rows_mark_orphans_by_process_start_and_age(pool: PgPool) -> sqlx::Result<()> {
        let s = started(&pool).await;
        let now = Utc::now();
        let young = pending_withdrawals(&pool, Some(now - Duration::hours(1))).await?;
        assert_eq!(young.len(), 1);
        assert!(!young[0].orphan, "newer than the process, and young");
        assert!(young[0].sent.is_none());
        assert!(young[0].signature.is_none());
        let none = pending_withdrawals(&pool, None).await?;
        assert!(!none[0].orphan);
        let before = pending_withdrawal(&pool, s.id, Some(now + Duration::seconds(1)))
            .await?
            .expect("row");
        assert!(before.orphan, "created before this process started");
        let listed = pending_withdrawals(&pool, Some(now + Duration::seconds(1))).await?;
        assert!(listed[0].orphan, "the list agrees");
        // Age alone: push the row back past ORPHAN_AFTER_SECS.
        sqlx::query!(
            "UPDATE coin_withdrawals SET created_at = now() - make_interval(secs => $2::float8)
             WHERE id = $1",
            s.id,
            (ORPHAN_AFTER_SECS + 5) as f64
        )
        .execute(&pool)
        .await?;
        let old = pending_withdrawal(&pool, s.id, None).await?.expect("row");
        assert!(old.orphan, "older than ORPHAN_AFTER_SECS");
        assert!(
            pending_withdrawals(&pool, None).await?[0].orphan,
            "the list agrees"
        );
        // Resolved rows are not pending.
        record_sent(&pool, s.id, &rec("sigO", &[1], false)).await?;
        finish_withdrawal(&pool, s.id).await?;
        assert!(pending_withdrawals(&pool, None).await?.is_empty());
        assert!(pending_withdrawal(&pool, s.id, None).await?.is_none());
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn sponsored_counts_only_done_rows_that_created_an_account(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        fund(&pool, G, U, 1000).await;
        fund(&pool, G2, U, 1000).await;
        // done + sponsored: counts.
        let a = start_withdrawal(&pool, &new_w(G, U, 1)).await.unwrap();
        record_sent(&pool, a.id, &rec("s1", &[1], true)).await?;
        finish_withdrawal(&pool, a.id).await?;
        assert_eq!(sponsored_accounts(&pool, U, MINT).await?, 1);
        // done, not sponsored: no.
        let b = start_withdrawal(&pool, &new_w(G, U, 1)).await.unwrap();
        record_sent(&pool, b.id, &rec("s2", &[1], false)).await?;
        finish_withdrawal(&pool, b.id).await?;
        assert_eq!(sponsored_accounts(&pool, U, MINT).await?, 1);
        // refunded + sponsored: no.
        let c = start_withdrawal(&pool, &new_w(G, U, 1)).await.unwrap();
        record_sent(&pool, c.id, &rec("s3", &[1], true)).await?;
        refund_withdrawal(&pool, c.id, "expired").await?;
        assert_eq!(sponsored_accounts(&pool, U, MINT).await?, 1);
        // another mint: no.
        let d = start_withdrawal(
            &pool,
            &NewWithdrawal {
                mint: "OtherMint",
                ..new_w(G, U, 1)
            },
        )
        .await
        .unwrap();
        record_sent(&pool, d.id, &rec("s4", &[1], true)).await?;
        finish_withdrawal(&pool, d.id).await?;
        assert_eq!(sponsored_accounts(&pool, U, MINT).await?, 1);
        assert_eq!(sponsored_accounts(&pool, U, "OtherMint").await?, 1);
        // pending + sponsored: not yet.
        let e = start_withdrawal(&pool, &new_w(G2, U, 1)).await.unwrap();
        record_sent(&pool, e.id, &rec("s5", &[1], true)).await?;
        assert_eq!(sponsored_accounts(&pool, U, MINT).await?, 1);
        // another member: no.
        assert_eq!(sponsored_accounts(&pool, UserId::new(11), MINT).await?, 0);
        finish_withdrawal(&pool, e.id).await?;
        assert_eq!(
            sponsored_accounts(&pool, U, MINT).await?,
            2,
            "across servers too"
        );
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn link_replaces_and_unlink_deletes(pool: PgPool) -> sqlx::Result<()> {
        assert_eq!(linked_address(&pool, U).await?, None);
        link_address(&pool, U, "AAA").await?;
        assert_eq!(linked_address(&pool, U).await?.as_deref(), Some("AAA"));
        link_address(&pool, U, "BBB").await?;
        assert_eq!(linked_address(&pool, U).await?.as_deref(), Some("BBB"));
        assert_eq!(linked_address(&pool, UserId::new(11)).await?, None);
        assert!(unlink_address(&pool, U).await?);
        assert!(!unlink_address(&pool, U).await?);
        assert_eq!(linked_address(&pool, U).await?, None);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn overview_sums_done_amounts_and_shows_the_last(pool: PgPool) -> sqlx::Result<()> {
        let none = wallet_overview(&pool, U).await?;
        assert_eq!(none.total_withdrawn, 0);
        assert!(none.last.is_none());
        fund(&pool, G, U, 100).await;
        let a = start_withdrawal(&pool, &new_w(G, U, 10)).await.unwrap();
        record_sent(&pool, a.id, &rec("sigA", &[1], false)).await?;
        finish_withdrawal(&pool, a.id).await?;
        let b = start_withdrawal(&pool, &new_w(G, U, 20)).await.unwrap();
        record_sent(&pool, b.id, &rec("sigB", &[1], false)).await?;
        finish_withdrawal(&pool, b.id).await?;
        let c = start_withdrawal(&pool, &new_w(G, U, 5)).await.unwrap();
        record_sent(&pool, c.id, &rec("sigC", &[1], false)).await?;
        refund_withdrawal(&pool, c.id, "devnet was reset").await?;
        let o = wallet_overview(&pool, U).await?;
        assert_eq!(o.total_withdrawn, 30, "only done rows");
        let last = o.last.expect("a last withdrawal");
        assert_eq!(last.amount, 5);
        assert_eq!(last.state, WithdrawalState::Refunded);
        assert_eq!(last.reason.as_deref(), Some("devnet was reset"));
        assert_eq!(last.signature.as_deref(), Some("sigC"));
        // A pending one is the last, with its signature once recorded.
        let d = start_withdrawal(&pool, &new_w(G, U, 7)).await.unwrap();
        record_sent(&pool, d.id, &rec("sigD", &[1], false)).await?;
        let last = wallet_overview(&pool, U).await?.last.expect("last");
        assert_eq!((last.amount, last.state), (7, WithdrawalState::Pending));
        assert_eq!(last.signature.as_deref(), Some("sigD"));
        assert_eq!(
            wallet_overview(&pool, U).await?.total_withdrawn,
            30,
            "a pending row is not withdrawn yet"
        );
        assert!(last.created_at <= Utc::now());
        Ok(())
    }

    #[test]
    fn withdrawal_state_text_round_trips() {
        for st in [
            WithdrawalState::Pending,
            WithdrawalState::Done,
            WithdrawalState::Refunded,
        ] {
            assert_eq!(WithdrawalState::parse(st.as_str()), Some(st));
        }
        assert_eq!(WithdrawalState::parse("nope"), None);
    }
}
