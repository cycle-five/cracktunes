//! Buying things with credits: the checks, the charge, the grant, and the
//! refund when the grant fails. Immunity is the first item for sale.

use super::immunity::Protection;
use super::settings::{ActionKind, ActionPrice};
use super::wallet::{ActionPayload, MoveError};
use crate::messaging::messages::{
    ECONOMY_ALREADY_IMMUNE, ECONOMY_ALREADY_RUNNING_PREFIX, ECONOMY_BUYER_NOT_IN_VOICE,
    ECONOMY_BUYER_OPTED_OUT, ECONOMY_CANNOT_JOIN_DESTINATION, ECONOMY_CANT_AFFORD_MIDDLE,
    ECONOMY_CANT_AFFORD_PREFIX, ECONOMY_NOT_FOR_SALE, ECONOMY_NO_JAIL_CHANNEL, ECONOMY_OFF,
    ECONOMY_OUT_OF_REACH, ECONOMY_TARGET_IMMUNE_INDEFINITELY, ECONOMY_TARGET_IMMUNE_UNTIL,
    ECONOMY_TARGET_IS_BOT, ECONOMY_TARGET_IS_SELF, ECONOMY_TARGET_NOT_IN_VOICE,
    ECONOMY_TARGET_OPTED_OUT, ECONOMY_UNAVAILABLE_PREFIX, ECONOMY_UNAVAILABLE_SUFFIX,
};
use crate::Data;
use chrono::{DateTime, Utc};
use serenity::all::{GuildId, Permissions, UserId};

/// Everything [`check_purchase`] needs, read up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurchaseFacts {
    pub enabled: bool,
    pub price: ActionPrice,
    /// The action's own switch (for immunity, `immunity_shop`).
    pub source_on: bool,
    pub buyer_opted_out: bool,
    /// The buyer holds a moderator grant that lasts until lifted, so a
    /// purchase would add nothing.
    pub already_immune_indefinitely: bool,
    pub balance: i64,
}

/// Why a purchase was refused before any credits moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    EconomyOff,
    NotForSale,
    BuyerOptedOut,
    AlreadyImmune,
    CantAfford {
        short: i64,
    },
    TargetIsBot,
    TargetIsSelf,
    OutOfReach,
    TargetNotInVoice,
    BuyerNotInVoice,
    TargetOptedOut,
    TargetImmune(Protection),
    AlreadyRunning {
        until: DateTime<Utc>,
    },
    CannotJoinDestination,
    NoJailChannel,
    /// Preflight: CrackTunes lacks these permissions for the action.
    Unavailable {
        missing: Permissions,
    },
}

/// Check a purchase, refusing for the first reason that applies, in order:
/// economy off, not for sale, buyer opted out, already immune indefinitely,
/// can't afford.
pub fn check_purchase(f: &PurchaseFacts) -> Result<(), Refusal> {
    if !f.enabled {
        return Err(Refusal::EconomyOff);
    }
    if !f.source_on || !f.price.on {
        return Err(Refusal::NotForSale);
    }
    if f.buyer_opted_out {
        return Err(Refusal::BuyerOptedOut);
    }
    if f.already_immune_indefinitely {
        return Err(Refusal::AlreadyImmune);
    }
    let price = i64::from(f.price.price);
    if f.balance < price {
        return Err(cant_afford(price, f.balance));
    }
    Ok(())
}

/// The refusal for a buyer holding `balance` against a `price`.
pub fn cant_afford(price: i64, balance: i64) -> Refusal {
    Refusal::CantAfford {
        short: price - balance,
    }
}

/// The words for a refusal, naming the server's currency.
pub fn refusal_text(r: &Refusal, currency: &str) -> String {
    match r {
        Refusal::EconomyOff => ECONOMY_OFF.to_string(),
        Refusal::NotForSale => ECONOMY_NOT_FOR_SALE.to_string(),
        Refusal::BuyerOptedOut => ECONOMY_BUYER_OPTED_OUT.to_string(),
        Refusal::AlreadyImmune => ECONOMY_ALREADY_IMMUNE.to_string(),
        Refusal::CantAfford { short } => {
            format!("{ECONOMY_CANT_AFFORD_PREFIX} {short} {ECONOMY_CANT_AFFORD_MIDDLE} {currency}.")
        },
        Refusal::TargetIsBot => ECONOMY_TARGET_IS_BOT.to_string(),
        Refusal::TargetIsSelf => ECONOMY_TARGET_IS_SELF.to_string(),
        Refusal::OutOfReach => ECONOMY_OUT_OF_REACH.to_string(),
        Refusal::TargetNotInVoice => ECONOMY_TARGET_NOT_IN_VOICE.to_string(),
        Refusal::BuyerNotInVoice => ECONOMY_BUYER_NOT_IN_VOICE.to_string(),
        Refusal::TargetOptedOut => ECONOMY_TARGET_OPTED_OUT.to_string(),
        Refusal::TargetImmune(Protection::Until(t)) => {
            format!("{ECONOMY_TARGET_IMMUNE_UNTIL} <t:{}:f>.", t.timestamp())
        },
        Refusal::TargetImmune(Protection::Indefinitely) => {
            ECONOMY_TARGET_IMMUNE_INDEFINITELY.to_string()
        },
        Refusal::TargetImmune(Protection::OptedOut) => ECONOMY_TARGET_OPTED_OUT.to_string(),
        Refusal::AlreadyRunning { until } => {
            format!(
                "{ECONOMY_ALREADY_RUNNING_PREFIX} <t:{}:R>.",
                until.timestamp()
            )
        },
        Refusal::CannotJoinDestination => ECONOMY_CANNOT_JOIN_DESTINATION.to_string(),
        Refusal::NoJailChannel => ECONOMY_NO_JAIL_CHANNEL.to_string(),
        Refusal::Unavailable { missing } => format!(
            "{ECONOMY_UNAVAILABLE_PREFIX} {} {ECONOMY_UNAVAILABLE_SUFFIX}",
            crate::rooms::card::perm_names(*missing, true)
        ),
    }
}

/// Why a purchase did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyError {
    Refused(Refusal),
    NoDatabase,
    Db,
}

/// A completed immunity purchase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImmunityBought {
    pub until: DateTime<Utc>,
    pub balance: i64,
}

/// Buy immunity: check, charge (recorded `pending`), then grant and mark the
/// action `done` in one transaction. A failed grant is refunded.
///
/// A row left `pending` was never granted: the grant and `done` commit
/// together. Reconcile (PR 2) may refund any pending immunity row.
pub async fn buy_immunity(
    data: &Data,
    g: GuildId,
    buyer: UserId,
    now: DateTime<Utc>,
) -> Result<ImmunityBought, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let settings = data.economy.settings(g);
    let cfg = &settings.config;
    let price = cfg.prices.get(ActionKind::Immunity);
    let balance = db::balance(pool, g, buyer).await.map_err(|err| {
        tracing::warn!("economy: reading balance of {buyer} in {g} failed: {err}");
        BuyError::Db
    })?;
    let grants = db::grants(pool, g, buyer).await.map_err(|err| {
        tracing::warn!("economy: reading grants of {buyer} in {g} failed: {err}");
        BuyError::Db
    })?;
    let facts = PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price,
        source_on: cfg.immunity_shop,
        buyer_opted_out: data.economy.is_opted_out(g, buyer),
        already_immune_indefinitely: grants.iter().any(|gr| gr.until.is_none()),
        balance,
    };
    check_purchase(&facts).map_err(BuyError::Refused)?;

    let hours = cfg.bought_immunity_hours;
    let payload = ActionPayload::Immunity { hours };
    let amount = i64::from(price.price);
    let charged = db::charge_for_action(
        pool,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Immunity,
            buyer,
            target: None,
            price: amount,
            note: None,
            signed: false,
            payload: &payload,
            expires_at: None,
        },
    )
    .await
    .map_err(|err| match err {
        // The balance moved between the check and the charge.
        MoveError::Insufficient { balance } => BuyError::Refused(cant_afford(amount, balance)),
        MoveError::Invalid => BuyError::Db,
        MoveError::Db(err) => {
            tracing::warn!("economy: charging {buyer} in {g} failed: {err}");
            BuyError::Db
        },
    })?;

    let until =
        match db::grant_bought_for_action(pool, charged.action_id, g, buyer, hours, now).await {
            Ok(until) => until,
            Err(err) => {
                tracing::warn!("economy: granting immunity to {buyer} in {g} failed: {err}");
                // Refund moves the action to failed and restores the credits.
                if let Err(err) = db::refund_action(pool, charged.action_id).await {
                    tracing::error!(
                        "economy: refunding action {} failed: {err}",
                        charged.action_id
                    );
                }
                return Err(BuyError::Db);
            },
        };
    Ok(ImmunityBought {
        until,
        balance: charged.balance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::immunity::Protection;
    use crate::economy::settings::EconomySettings;
    use crate::DataInner;
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(10);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn ok() -> PurchaseFacts {
        PurchaseFacts {
            enabled: true,
            price: ActionPrice {
                price: 200,
                on: true,
            },
            source_on: true,
            buyer_opted_out: false,
            already_immune_indefinitely: false,
            balance: 1000,
        }
    }

    #[test]
    fn check_purchase_order() {
        assert_eq!(check_purchase(&ok()), Ok(()));
        // Everything wrong at once: economy off wins, then each falls away.
        let mut f = PurchaseFacts {
            enabled: false,
            source_on: false,
            buyer_opted_out: true,
            already_immune_indefinitely: true,
            balance: 0,
            ..ok()
        };
        assert_eq!(check_purchase(&f), Err(Refusal::EconomyOff));
        f.enabled = true;
        assert_eq!(check_purchase(&f), Err(Refusal::NotForSale));
        f.source_on = true;
        f.price.on = false;
        assert_eq!(check_purchase(&f), Err(Refusal::NotForSale));
        f.price.on = true;
        assert_eq!(check_purchase(&f), Err(Refusal::BuyerOptedOut));
        f.buyer_opted_out = false;
        assert_eq!(check_purchase(&f), Err(Refusal::AlreadyImmune));
        f.already_immune_indefinitely = false;
        assert_eq!(check_purchase(&f), Err(Refusal::CantAfford { short: 200 }));
        f.balance = 200;
        assert_eq!(check_purchase(&f), Ok(()));
    }

    #[test]
    fn cant_afford_says_how_many_more() {
        let f = PurchaseFacts {
            balance: 160,
            ..ok()
        };
        let r = check_purchase(&f).unwrap_err();
        assert_eq!(r, Refusal::CantAfford { short: 40 });
        assert_eq!(refusal_text(&r, "credits"), "You need 40 more credits.");
    }

    #[test]
    fn cant_afford_is_price_minus_balance() {
        assert_eq!(cant_afford(200, 50), Refusal::CantAfford { short: 150 });
        assert_eq!(cant_afford(200, 0), Refusal::CantAfford { short: 200 });
    }

    #[test]
    fn every_refusal_has_its_own_text() {
        let all = [
            Refusal::EconomyOff,
            Refusal::NotForSale,
            Refusal::BuyerOptedOut,
            Refusal::AlreadyImmune,
            Refusal::CantAfford { short: 1 },
            Refusal::TargetIsBot,
            Refusal::TargetIsSelf,
            Refusal::OutOfReach,
            Refusal::TargetNotInVoice,
            Refusal::BuyerNotInVoice,
            Refusal::TargetOptedOut,
            Refusal::TargetImmune(Protection::Until(t0())),
            Refusal::TargetImmune(Protection::Indefinitely),
            Refusal::AlreadyRunning { until: t0() },
            Refusal::CannotJoinDestination,
            Refusal::NoJailChannel,
            Refusal::Unavailable {
                missing: Permissions::MOVE_MEMBERS,
            },
        ];
        let texts: std::collections::HashSet<_> =
            all.iter().map(|r| refusal_text(r, "credits")).collect();
        assert_eq!(texts.len(), all.len());
        assert_eq!(refusal_text(&Refusal::EconomyOff, "x"), ECONOMY_OFF);
    }

    #[test]
    fn target_refusals_carry_their_facts() {
        let t = t0().timestamp();
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::Until(t0())), "c"),
            format!("They're immune until <t:{t}:f>.")
        );
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::Indefinitely), "c"),
            "They're immune until a moderator lifts it."
        );
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::OptedOut), "c"),
            "They've opted out of the economy."
        );
        assert_eq!(
            refusal_text(&Refusal::AlreadyRunning { until: t0() }, "c"),
            format!("They're already pranked like that until <t:{t}:R>.")
        );
        let text = refusal_text(
            &Refusal::Unavailable {
                missing: Permissions::MOVE_MEMBERS | Permissions::MUTE_MEMBERS,
            },
            "c",
        );
        assert!(text.starts_with("CrackTunes is missing "), "{text}");
        assert!(text.ends_with(" for that."), "{text}");
        assert!(
            text.contains("Move Members") && text.contains("Mute Members"),
            "{text}"
        );
    }

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    async fn shop_data(pool: &PgPool, funds: i64) -> Data {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        crate::db::economy::save_settings(pool, &s).await.unwrap();
        if funds > 0 {
            crate::db::economy::credit_earnings(
                pool,
                G,
                &[crate::db::economy::EarnRow {
                    user: U,
                    voice: funds,
                    messages: 0,
                }],
            )
            .await
            .unwrap();
        }
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data, G).await);
        data
    }

    async fn count(pool: &PgPool, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    macro_rules! db_test {
        ($name:ident, |$pool:ident: PgPool| $body:expr) => {
            #[sqlx::test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) {
                $body.await
            }
        };
    }

    db_test!(buying_immunity_charges_and_protects, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let now = t0();
        let bought = buy_immunity(&data, G, U, now).await.unwrap();
        assert_eq!(bought.until, now + chrono::Duration::hours(24));
        assert_eq!(bought.balance, 300);
        assert_eq!(
            crate::economy::protection(&data, G, U, now).await.unwrap(),
            Some(Protection::Until(now + chrono::Duration::hours(24)))
        );
        assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 300);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE delta = -200 AND kind <> 'refund'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'done'"
            )
            .await,
            1
        );
    });

    db_test!(buying_twice_extends, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let now = t0();
        buy_immunity(&data, G, U, now).await.unwrap();
        let again = buy_immunity(&data, G, U, now).await.unwrap();
        assert_eq!(again.until, now + chrono::Duration::hours(48));
        assert_eq!(again.balance, 100);
    });

    db_test!(
        a_broke_buyer_is_refused_and_nothing_moves,
        |pool: PgPool| async {
            let data = shop_data(&pool, 160).await;
            assert_eq!(
                buy_immunity(&data, G, U, t0()).await,
                Err(BuyError::Refused(Refusal::CantAfford { short: 40 }))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 160);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions").await,
                0
            );
        }
    );

    db_test!(
        two_buys_racing_for_one_purchase_of_funds,
        |pool: PgPool| async {
            // 250 buys one immunity. Whether the loser is stopped by the check or
            // by the charge (the race), it is told the same shortfall.
            let data = shop_data(&pool, 250).await;
            let now = t0();
            let (a, b) = tokio::join!(
                buy_immunity(&data, G, U, now),
                buy_immunity(&data, G, U, now)
            );
            let (won, lost) = if a.is_ok() { (a, b) } else { (b, a) };
            assert_eq!(won.unwrap().balance, 50);
            assert_eq!(
                lost,
                Err(BuyError::Refused(Refusal::CantAfford { short: 150 }))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 50);
        }
    );

    db_test!(
        an_indefinite_mod_grant_refuses_a_purchase_before_any_charge,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            crate::db::economy::grant_mod(&pool, G, U, None, UserId::new(99), None)
                .await
                .unwrap();
            assert_eq!(
                buy_immunity(&data, G, U, t0()).await,
                Err(BuyError::Refused(Refusal::AlreadyImmune))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 500);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions").await,
                0
            );
        }
    );

    db_test!(grant_and_done_commit_together, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        buy_immunity(&data, G, U, t0()).await.unwrap();
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'done' AND ended_at IS NOT NULL"
            )
            .await,
            1
        );
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_immunity").await,
            1
        );
    });

    db_test!(
        granting_a_finished_action_grants_nothing,
        |pool: PgPool| async {
            use crate::db::economy as db;
            use crate::economy::wallet::ActionPayload;
            let data = shop_data(&pool, 500).await;
            let payload = ActionPayload::Immunity { hours: 24 };
            let charged = db::charge_for_action(
                &pool,
                &db::NewAction {
                    guild: G,
                    kind: ActionKind::Immunity,
                    buyer: U,
                    target: None,
                    price: 200,
                    note: None,
                    signed: false,
                    payload: &payload,
                    expires_at: None,
                },
            )
            .await
            .unwrap();
            db::refund_action(&pool, charged.action_id).await.unwrap();
            let r = db::grant_bought_for_action(&pool, charged.action_id, G, U, 24, t0()).await;
            assert!(r.is_err());
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_immunity").await,
                0
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
            let _ = data;
        }
    );

    db_test!(a_failed_grant_is_refunded, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        // Reads of grants still work (the check reads them); only the grant fails.
        sqlx::query(
            "CREATE FUNCTION refuse_grant() RETURNS trigger AS $$ BEGIN RAISE EXCEPTION 'no'; END $$ LANGUAGE plpgsql",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TRIGGER refuse_grant BEFORE INSERT ON economy_immunity FOR EACH ROW EXECUTE FUNCTION refuse_grant()",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(buy_immunity(&data, G, U, t0()).await, Err(BuyError::Db));
        assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'failed'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'refund'"
            )
            .await,
            1
        );
    });

    #[tokio::test]
    async fn without_a_pool_buying_says_no_database() {
        let data = Data(Arc::new(DataInner::default()));
        assert_eq!(
            buy_immunity(&data, G, U, t0()).await,
            Err(BuyError::NoDatabase)
        );
    }
}
