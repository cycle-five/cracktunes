//! Walk-on themes: a member pays to have a link play, for a few seconds, the
//! next time another member joins voice. This is the purchase; playing it is
//! done on the join. The credits stay in escrow until then and come back if
//! it never plays.

use super::settings::ActionKind;
use super::shop::{
    cant_afford, check_purchase, Bought, BuyError, BuyOutcome, PurchaseFacts, Refusal,
};
use super::target::{check_target, TargetFacts};
use super::wallet::{ActionPayload, LedgerKind, MoveError};
use crate::db::economy as db;
use crate::Data;
use chrono::{DateTime, Duration as Days, Utc};
use serenity::all::{GuildId, UserId};
use std::time::Duration;

/// How long a theme waits for its target's next join.
pub const THEME_DAYS: i64 = 7;

/// The most of the link that plays.
pub const THEME_CLIP: Duration = Duration::from_secs(10);

/// The latest the clip may start into the link: twelve hours.
pub const THEME_START_MAX: Duration = Duration::from_secs(12 * 60 * 60);

/// The longest link accepted, in characters.
pub const LINK_MAX_CHARS: usize = 512;

/// Why a start time was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    /// Not `SS`, `M:SS` or `H:MM:SS`.
    Malformed,
    /// Later than [`THEME_START_MAX`].
    TooLate,
}

/// Parse a start time: `SS`, `M:SS` or `H:MM:SS`. Every field after the first
/// is below 60. Whole seconds; at most [`THEME_START_MAX`].
pub fn parse_start(raw: &str) -> Result<Duration, StartError> {
    let fields: Vec<&str> = raw.trim().split(':').collect();
    if fields.len() > 3 {
        return Err(StartError::Malformed);
    }
    let mut secs: u64 = 0;
    for (i, f) in fields.iter().enumerate() {
        // Digits only: no sign, no space, no empty field. The length cap
        // keeps a huge first field a "too late", never an overflow.
        if f.is_empty() || f.len() > 9 || !f.bytes().all(|b| b.is_ascii_digit()) {
            return Err(StartError::Malformed);
        }
        let n: u64 = f.parse().map_err(|_| StartError::Malformed)?;
        if i > 0 && n >= 60 {
            return Err(StartError::Malformed);
        }
        secs = secs * 60 + n;
    }
    let start = Duration::from_secs(secs);
    if start > THEME_START_MAX {
        return Err(StartError::TooLate);
    }
    Ok(start)
}

/// The link as stored: trimmed, and one yt-dlp would be handed. Over
/// [`LINK_MAX_CHARS`] or refused by `ytdl_for_url` is [`Refusal::BadLink`].
pub fn check_link(raw: &str) -> Result<String, Refusal> {
    let url = raw.trim();
    if url.is_empty() || url.chars().count() > LINK_MAX_CHARS {
        return Err(Refusal::BadLink);
    }
    // Only whether it parses matters here: the built input is dropped.
    crack_types::ytdl_for_url(crate::http_utils::get_client_old().clone(), url)
        .map(drop)
        .map_err(|_| Refusal::BadLink)?;
    Ok(url.to_string())
}

/// Check a theme, refusing for the first reason that applies: the shop's own
/// reasons (off, not for sale, buyer opted out, can't afford), then the link,
/// then the target (bot, self, opted out, immune, a theme already waiting).
/// Returns the link to store.
pub fn check_theme(
    purchase: &PurchaseFacts,
    url: &str,
    target: &TargetFacts,
) -> Result<String, Refusal> {
    check_purchase(purchase)?;
    let url = check_link(url)?;
    check_target(ActionKind::Theme, target, None)?;
    Ok(url)
}

/// What to buy.
#[derive(Debug, Clone)]
pub struct ThemeRequest {
    pub guild: GuildId,
    pub buyer: UserId,
    pub target: UserId,
    pub url: String,
    pub start: Duration,
    pub note: Option<String>,
    pub signed: bool,
    pub now: DateTime<Utc>,
}

/// Buy a theme: check, then escrow the price in one transaction, the row
/// `active` until [`THEME_DAYS`] from now. `facts` describes the target. The
/// caller validates the note and posts the mod log event.
pub async fn buy_theme(
    data: &Data,
    facts: TargetFacts,
    req: ThemeRequest,
) -> Result<Bought, BuyError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let (g, buyer) = (req.guild, req.buyer);
    let settings = data.economy.settings(g);
    let price = settings.config.prices.get(ActionKind::Theme);
    let balance = db::balance(pool, g, buyer).await.map_err(|err| {
        tracing::warn!("economy: reading balance of {buyer} in {g} failed: {err}");
        BuyError::Db
    })?;
    let url = check_theme(
        &PurchaseFacts {
            enabled: data.economy.is_enabled(g),
            price,
            source_on: true,
            buyer_opted_out: data.economy.is_opted_out(g, buyer),
            already_immune_indefinitely: false,
            balance,
        },
        &req.url,
        &facts,
    )
    .map_err(BuyError::Refused)?;
    let until = req.now + Days::days(THEME_DAYS);
    let price = i64::from(price.price);
    let charged = db::charge_active(
        pool,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Theme,
            buyer,
            target: Some(req.target),
            price,
            note: req.note.as_deref(),
            signed: req.signed,
            payload: &ActionPayload::Theme {
                url,
                start_secs: u32::try_from(req.start.as_secs()).unwrap_or(u32::MAX),
            },
            expires_at: Some(until),
        },
        LedgerKind::Purchase,
    )
    .await
    .map_err(|err| match err {
        // The balance moved between the check and the charge.
        MoveError::Insufficient { balance } => BuyError::Refused(cant_afford(price, balance)),
        MoveError::Invalid => BuyError::Db,
        MoveError::Db(err) => {
            tracing::warn!("economy: escrowing a theme from {buyer} in {g} failed: {err}");
            BuyError::Db
        },
    })?;
    Ok(Bought {
        action_id: charged.action_id,
        balance: charged.balance,
        outcome: BuyOutcome::Landed,
        until: Some(until),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::immunity::Protection;
    use crate::economy::settings::{ActionPrice, EconomySettings};
    use crate::DataInner;
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const A: UserId = UserId::new(10);
    const T: UserId = UserId::new(20);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    const LINK: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn parse_start_edges() {
        for (raw, want) in [
            ("0", 0),
            ("83", 83),
            ("1:23", 83),
            ("1:02:03", 3723),
            ("12:00:00", 43_200),
            (" 7 ", 7),
            ("00:05", 5),
        ] {
            assert_eq!(parse_start(raw), Ok(secs(want)), "{raw:?}");
        }
        for raw in ["12:00:01", "43201", "721:00", "999999999"] {
            assert_eq!(parse_start(raw), Err(StartError::TooLate), "{raw:?}");
        }
        for raw in [
            "1:60",
            "1:2:60",
            "a",
            "",
            "1::2",
            "-5",
            "+5",
            "1.5",
            "1:2:3:4",
            ":30",
            "1:",
            "1 2",
            "9999999999",
        ] {
            assert_eq!(parse_start(raw), Err(StartError::Malformed), "{raw:?}");
        }
    }

    fn purchase() -> PurchaseFacts {
        PurchaseFacts {
            enabled: true,
            price: ActionPrice {
                price: 100,
                on: true,
            },
            source_on: true,
            buyer_opted_out: false,
            already_immune_indefinitely: false,
            balance: 1000,
        }
    }

    fn clean() -> TargetFacts {
        TargetFacts {
            is_bot: false,
            is_self: false,
            is_owner: false,
            top_role_position: Some(1),
            bot_top_role_position: Some(5),
            in_voice: None,
            server_mute: false,
            server_deaf: false,
            opted_out: false,
            protection: None,
            has_mirror: false,
            running_same_kind: None,
            can_join_destination: None,
            current_nick: None,
            display_name: "t".to_string(),
        }
    }

    #[test]
    fn theme_refusals() {
        assert_eq!(
            check_theme(&purchase(), &format!("  {LINK}\n"), &clean()),
            Ok(LINK.to_string()),
            "trimmed, and a good link passes"
        );
        // A link yt-dlp would not be handed.
        for bad in [
            "file:///etc/passwd",
            "ftp://x",
            "not a link",
            "",
            "   ",
            "-o /tmp/x",
        ] {
            assert_eq!(
                check_theme(&purchase(), bad, &clean()),
                Err(Refusal::BadLink),
                "{bad:?}"
            );
        }
        // The length cap: 512 chars pass, 513 do not.
        let pad = |n: usize| format!("https://example.com/{}", "a".repeat(n - 20));
        assert_eq!(pad(512).chars().count(), 512);
        assert!(check_theme(&purchase(), &pad(512), &clean()).is_ok());
        assert_eq!(
            check_theme(&purchase(), &pad(513), &clean()),
            Err(Refusal::BadLink)
        );
        // The shop's reasons come first, then the link, then the target.
        let broke = PurchaseFacts {
            balance: 5,
            ..purchase()
        };
        assert_eq!(
            check_theme(&broke, "ftp://x", &clean()),
            Err(Refusal::CantAfford { short: 95 })
        );
        let me = TargetFacts {
            is_self: true,
            ..clean()
        };
        assert_eq!(
            check_theme(&purchase(), "ftp://x", &me),
            Err(Refusal::BadLink)
        );
        assert_eq!(
            check_theme(&purchase(), LINK, &me),
            Err(Refusal::TargetIsSelf)
        );
        let immune = TargetFacts {
            protection: Some(Protection::Indefinitely),
            ..clean()
        };
        assert_eq!(
            check_theme(&purchase(), LINK, &immune),
            Err(Refusal::TargetImmune(Protection::Indefinitely))
        );
        // A second theme waits for the first, and is told when that ends.
        let end = Utc::now() + Days::days(3);
        let busy = TargetFacts {
            running_same_kind: Some(end),
            ..clean()
        };
        assert_eq!(
            check_theme(&purchase(), LINK, &busy),
            Err(Refusal::AlreadyRunning { until: end })
        );
        // No mirror, no reach, no voice needed.
        let odd = TargetFacts {
            has_mirror: true,
            is_owner: true,
            top_role_position: Some(9),
            ..clean()
        };
        assert!(check_theme(&purchase(), LINK, &odd).is_ok());
        assert_eq!(
            crate::economy::shop::refusal_text(&Refusal::BadLink, "credits"),
            "That link can't be played. Use an http(s) link to a song or video."
        );
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

    async fn themed_data(pool: &PgPool) -> Data {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        db::save_settings(pool, &s).await.unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data, G).await);
        data
    }

    fn request(url: &str, now: DateTime<Utc>) -> ThemeRequest {
        ThemeRequest {
            guild: G,
            buyer: A,
            target: T,
            url: url.to_string(),
            start: secs(83),
            note: Some("hi".into()),
            signed: true,
            now,
        }
    }

    async fn count(pool: &PgPool, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    db_test!(a_theme_purchase_escrows_and_waits, |pool: PgPool| async {
        let data = themed_data(&pool).await;
        db::credit_earnings(
            &pool,
            G,
            &[db::EarnRow {
                user: A,
                voice: 250,
                messages: 0,
            }],
        )
        .await
        .unwrap();
        // The real clock: the database compares the end with now().
        let now = Utc::now();
        let bought = buy_theme(&data, clean(), request(&format!(" {LINK} "), now))
            .await
            .unwrap();
        let until = now + Days::days(7);
        assert_eq!(bought.balance, 150);
        assert_eq!(bought.outcome, BuyOutcome::Landed);
        assert_eq!(bought.until, Some(until));
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 150);
        // It waits: active, aimed at the target, ending in seven days, with
        // the trimmed link and the start in whole seconds.
        let rows = db::active_of_kind(&pool, G, T, &[ActionKind::Theme])
            .await
            .unwrap();
        let [r] = rows.as_slice() else {
            panic!("{rows:?}")
        };
        assert_eq!(r.id, bought.action_id);
        assert_eq!((r.buyer, r.target), (A, Some(T)));
        assert_eq!(r.state, crate::economy::wallet::ActionState::Active);
        assert_eq!(
            r.expires_at.map(|e| e.timestamp_micros()),
            Some(until.timestamp_micros())
        );
        assert_eq!(r.note.as_deref(), Some("hi"));
        assert!(r.signed);
        assert_eq!(
            r.payload,
            ActionPayload::Theme {
                url: LINK.to_string(),
                start_secs: 83
            }
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'purchase' AND delta = -100"
            )
            .await,
            1
        );
        // Refused before any charge: a bad link, an immune target, a second
        // theme (with the first one's end), and a wallet too thin.
        assert_eq!(
            buy_theme(&data, clean(), request("ftp://x", now)).await,
            Err(BuyError::Refused(Refusal::BadLink))
        );
        let immune = TargetFacts {
            protection: Some(Protection::Indefinitely),
            ..clean()
        };
        assert_eq!(
            buy_theme(&data, immune, request(LINK, now)).await,
            Err(BuyError::Refused(Refusal::TargetImmune(
                Protection::Indefinitely
            )))
        );
        let busy = TargetFacts {
            running_same_kind: Some(until),
            ..clean()
        };
        assert_eq!(
            buy_theme(&data, busy, request(LINK, now)).await,
            Err(BuyError::Refused(Refusal::AlreadyRunning { until }))
        );
        buy_theme(
            &data,
            TargetFacts { ..clean() },
            ThemeRequest {
                target: UserId::new(21),
                ..request(LINK, now)
            },
        )
        .await
        .unwrap();
        assert_eq!(
            buy_theme(
                &data,
                clean(),
                ThemeRequest {
                    target: UserId::new(22),
                    ..request(LINK, now)
                }
            )
            .await,
            Err(BuyError::Refused(Refusal::CantAfford { short: 50 }))
        );
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 50);
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            2
        );
    });
}
