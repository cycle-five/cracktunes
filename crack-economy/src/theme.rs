//! Walk-on themes: a member pays to have a link play, for a few seconds, the
//! next time another member joins voice. The purchase escrows the price; the
//! target's next join plays the clip (crack-core's `economy::walk_on`, in
//! `on_voice_join`) and spends it. If it never plays, the buyer gets it back.

use super::immunity::Protection;
use super::settings::ActionKind;
use super::shop::{
    cant_afford, check_purchase, Bought, BuyError, BuyOutcome, PurchaseFacts, Refusal,
};
use super::target::{check_target, TargetFacts};
use super::wallet::{ActionPayload, ActionState, LedgerKind, MoveError};
use crate::db::economy::{self as db, ActionRow};
use crate::EconomyData;
use chrono::{DateTime, Duration as Days, Utc};
use serenity::all::{ChannelId, GuildId, UserId};
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{timeout_at, Instant};

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
    crack_types::ytdl_url(url).map_err(|_| Refusal::BadLink)?;
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
    data: &EconomyData,
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

// --- Playing it, on the target's next voice join ---------------------------

/// How long a started theme has to become audible.
pub const THEME_READY_WITHIN: Duration = Duration::from_secs(20);

/// An end this soon after becoming audible means nothing was heard (a seek
/// that landed at the end of the media opens, then ends at once).
pub const THEME_TOO_SOON: Duration = Duration::from_millis(500);

/// The channel a voice change plays a waiting theme in, if any: a join, or
/// the move out of the voice-rooms hub (`hub`, when rooms are on) into the
/// room made for them. A join into the hub plays nothing; the move follows.
/// Any other move, and a leave, play nothing.
pub fn theme_trigger(
    old: Option<ChannelId>,
    new: Option<ChannelId>,
    hub: Option<ChannelId>,
) -> Option<ChannelId> {
    let new = new?;
    match old {
        // A join, unless into the hub.
        None => (Some(new) != hub).then_some(new),
        // Out of the hub into another channel.
        Some(old) => (Some(old) == hub && old != new).then_some(new),
    }
}

/// Where a theme plays, decided when its target joins voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePlan {
    /// The bot is in the target's channel: mix the clip over whatever plays.
    MixHere,
    /// The bot is in no voice channel in the server: join, play, leave.
    JoinPlayLeave,
    /// Not now: the theme waits for a later join.
    Wait,
}

/// Decide the [`ThemePlan`]. A `/gp` game, another theme playing in the
/// server, or the bot busy in another channel make the theme wait.
pub fn theme_plan(
    bot_in: Option<ChannelId>,
    target_in: ChannelId,
    game_running: bool,
    playing_elsewhere: bool,
) -> ThemePlan {
    if game_running || playing_elsewhere {
        return ThemePlan::Wait;
    }
    match bot_in {
        None => ThemePlan::JoinPlayLeave,
        Some(here) if here == target_in => ThemePlan::MixHere,
        Some(_) => ThemePlan::Wait,
    }
}

/// The refund's reason when a theme can't reach its target now: `opt_out`
/// (the opt-out path's own reason) for a member out of the economy,
/// `immunity` for one immune. `None` when nothing protects them.
pub fn protected_reason(opted_out: bool, protection: Option<Protection>) -> Option<&'static str> {
    match (opted_out, protection) {
        (true, _) | (_, Some(Protection::OptedOut)) => Some("opt_out"),
        (_, Some(Protection::Until(_) | Protection::Indefinitely)) => Some("immunity"),
        (false, None) => None,
    }
}

/// Whether a claimed theme may start now: its row still `active`, and its
/// target still in the channel it is about to play in. Anything else is a
/// wait: nothing plays and nothing settles.
pub fn still_on(
    state: Option<ActionState>,
    target_in: Option<ChannelId>,
    channel: ChannelId,
) -> bool {
    state == Some(ActionState::Active) && target_in == Some(channel)
}

/// After a join-play-leave and its grace: leave only if nobody started
/// music meanwhile (the queue is empty), the bot is still where it joined,
/// no other theme is playing, and no `/gp` game runs (its submission window
/// plays nothing for minutes, and leaving would end it, as `IdleHandler`
/// knows).
pub fn leave_after_theme(
    queue_empty: bool,
    here: Option<ChannelId>,
    joined: ChannelId,
    theme_playing: bool,
    game_running: bool,
) -> bool {
    queue_empty && here == Some(joined) && !theme_playing && !game_running
}

/// What a theme's track tells the task playing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipEvent {
    Playable,
    Errored,
    Ended,
}

/// Why a theme did not play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unplayable {
    /// The track failed (yt-dlp, the stream, the decoder) or ended unheard.
    Failed,
    /// The seek to its start failed. songbird removes such a track.
    SeekFailed,
    /// Not audible within the time allowed.
    TimedOut,
}

/// Wait until the clip is audible at its start: the seek (when there is one)
/// confirmed, then `Playable` seen, both within `within` of the call. A
/// `Playable` that came before a failed seek does not count: songbird fires
/// one when the stream opens, before a seek queued behind it runs.
pub async fn audible<F, T, E>(
    seek: Option<F>,
    events: &mut mpsc::UnboundedReceiver<ClipEvent>,
    within: Duration,
) -> Result<(), Unplayable>
where
    F: Future<Output = Result<T, E>>,
{
    let deadline = Instant::now() + within;
    if let Some(seek) = seek {
        match timeout_at(deadline, seek).await {
            Ok(Ok(_)) => {},
            Ok(Err(_)) => return Err(Unplayable::SeekFailed),
            Err(_) => return Err(Unplayable::TimedOut),
        }
    }
    match timeout_at(deadline, events.recv()).await {
        Ok(Some(ClipEvent::Playable)) => Ok(()),
        Ok(Some(ClipEvent::Errored | ClipEvent::Ended) | None) => Err(Unplayable::Failed),
        Err(_) => Err(Unplayable::TimedOut),
    }
}

/// What settling a played theme found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// It was `active` and is now `done`: the credits are spent.
    Spent,
    /// It was no longer `active`: refunded meanwhile (a cleanse, a cancel,
    /// protection, its end).
    Gone,
    /// The write failed; the row is as it was.
    Unknown,
}

/// How an audible clip ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOutcome {
    /// Over within [`THEME_TOO_SOON`]: nothing was heard. Not settled.
    EndedAtOnce,
    /// Settling found it refunded, and it was stopped then.
    Refunded,
    /// It played. `ran_out`: it was stopped at [`THEME_CLIP`].
    Played { ran_out: bool, settled: Settled },
}

/// Ends when the clip does: an end, an error, or the track gone. A second
/// `Playable` (the one after a seek) is not an end.
async fn clip_over(events: &mut mpsc::UnboundedReceiver<ClipEvent>) {
    while let Some(ClipEvent::Playable) = events.recv().await {}
}

/// Run a clip that became audible at `audible_at`. An end within
/// [`THEME_TOO_SOON`] is [`ClipOutcome::EndedAtOnce`], and `settle` is never
/// started. Otherwise `settle` runs beside the clip, which is `stop`ped
/// [`THEME_CLIP`] after `audible_at` whatever the settle is doing, or at once
/// when the settle finds the row refunded. A settle still running when the
/// clip ends is awaited after the stop.
pub async fn run_clip<S>(
    events: &mut mpsc::UnboundedReceiver<ClipEvent>,
    audible_at: Instant,
    settle: S,
    mut stop: impl FnMut(),
) -> ClipOutcome
where
    S: Future<Output = Settled>,
{
    if timeout_at(audible_at + THEME_TOO_SOON, clip_over(events))
        .await
        .is_ok()
    {
        return ClipOutcome::EndedAtOnce;
    }
    let clip = timeout_at(audible_at + THEME_CLIP, clip_over(events));
    tokio::pin!(clip);
    tokio::pin!(settle);
    let mut settled = None;
    let ran_out = loop {
        tokio::select! {
            done = &mut settle, if settled.is_none() => {
                if done == Settled::Gone {
                    stop();
                    return ClipOutcome::Refunded;
                }
                settled = Some(done);
            },
            over = &mut clip => break over.is_err(),
        }
    };
    if ran_out {
        stop();
    }
    let settled = match settled {
        Some(done) => done,
        None => settle.await,
    };
    ClipOutcome::Played { ran_out, settled }
}

/// Steps 1-2 of a join: the target's oldest waiting theme, if it may play.
///
/// - Only `active` rows before their end count; one past it is the sweeper's
///   to refund. Duplicates (two bought at once) wait for later joins.
/// - A protected target (opted out, or immune) gets none: every theme waiting
///   for them is refunded to its buyer, and `None` returned.
/// - A read that fails plays nothing and refunds nothing.
pub async fn claim_theme(
    data: &EconomyData,
    guild: GuildId,
    user: UserId,
    now: DateTime<Utc>,
) -> Option<ActionRow> {
    let pool = data.database_pool.as_ref()?;
    let rows = match db::active_of_kind(pool, guild, user, &[ActionKind::Theme]).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(
                "economy: reading the themes waiting for {user} in {guild} failed: {err}"
            );
            return None;
        },
    };
    // Oldest first: the query orders by id.
    let waiting: Vec<ActionRow> = rows
        .into_iter()
        .filter(|r| r.state == ActionState::Active && r.expires_at.is_some_and(|end| end > now))
        .collect();
    let oldest = waiting.first()?.clone();
    let protection = match crate::protection(data, guild, user, now).await {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!("economy: reading the protection of {user} in {guild} failed: {err}");
            return None;
        },
    };
    let Some(reason) = protected_reason(data.economy.is_opted_out(guild, user), protection) else {
        return Some(oldest);
    };
    for row in &waiting {
        if let Err(err) = db::refund_active(pool, row.id, reason).await {
            tracing::warn!(
                "economy: refunding theme {} ({reason}) failed: {err}",
                row.id
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ActionPrice, EconomySettings};
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const A: UserId = UserId::new(10);
    const T: UserId = UserId::new(20);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

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
            crate::shop::refusal_text(&Refusal::BadLink, "credits"),
            "That link can't be played. Use an http(s) link to a song or video."
        );
    }

    macro_rules! db_test {
        ($name:ident, |$pool:ident: PgPool| $body:expr) => {
            #[crack_test_macros::db_test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) {
                $body.await
            }
        };
    }

    async fn themed_data(pool: &PgPool) -> EconomyData {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        db::save_settings(pool, &s).await.unwrap();
        let data = EconomyData {
            database_pool: Some(pool.clone()),
            ..Default::default()
        };
        assert!(crate::load_settings(&data, G).await);
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
        assert_eq!(r.state, crate::wallet::ActionState::Active);
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

    const VC: ChannelId = ChannelId::new(5);
    const OTHER_VC: ChannelId = ChannelId::new(6);

    #[test]
    fn theme_plan_a_game_or_another_theme_waits_first() {
        // Whatever the bot is doing, a game or another theme in the server
        // makes it wait.
        for bot_in in [None, Some(VC), Some(OTHER_VC)] {
            assert_eq!(theme_plan(bot_in, VC, true, false), ThemePlan::Wait);
            assert_eq!(theme_plan(bot_in, VC, false, true), ThemePlan::Wait);
            assert_eq!(theme_plan(bot_in, VC, true, true), ThemePlan::Wait);
        }
    }

    #[test]
    fn theme_plan_mixes_joins_or_waits_by_where_the_bot_is() {
        assert_eq!(theme_plan(Some(VC), VC, false, false), ThemePlan::MixHere);
        assert_eq!(theme_plan(None, VC, false, false), ThemePlan::JoinPlayLeave);
        assert_eq!(
            theme_plan(Some(OTHER_VC), VC, false, false),
            ThemePlan::Wait,
            "busy in another channel"
        );
    }

    #[test]
    fn protected_reason_names_the_protection() {
        let later = Utc::now() + Days::hours(1);
        assert_eq!(protected_reason(false, None), None);
        assert_eq!(protected_reason(true, None), Some("opt_out"), "in memory");
        assert_eq!(
            protected_reason(false, Some(Protection::OptedOut)),
            Some("opt_out"),
            "stored"
        );
        assert_eq!(
            protected_reason(false, Some(Protection::Until(later))),
            Some("immunity")
        );
        assert_eq!(
            protected_reason(false, Some(Protection::Indefinitely)),
            Some("immunity")
        );
        assert_eq!(
            protected_reason(true, Some(Protection::Indefinitely)),
            Some("opt_out")
        );
    }

    #[test]
    fn leave_after_theme_only_when_idle_and_where_it_joined() {
        assert!(leave_after_theme(true, Some(VC), VC, false, false));
        assert!(
            !leave_after_theme(false, Some(VC), VC, false, false),
            "music started"
        );
        assert!(
            !leave_after_theme(true, Some(OTHER_VC), VC, false, false),
            "moved"
        );
        assert!(
            !leave_after_theme(true, None, VC, false, false),
            "already gone"
        );
        assert!(
            !leave_after_theme(true, Some(VC), VC, true, false),
            "another theme is playing"
        );
        // A /gp game started on the theme's call: its submission window
        // queues nothing, and leaving would end the game.
        assert!(
            !leave_after_theme(true, Some(VC), VC, false, true),
            "a game runs"
        );
    }

    /// Sends `events` into a fresh channel at the given times (from now),
    /// then drops the sender unless `keep_open`.
    fn events_at(
        events: Vec<(u64, ClipEvent)>,
        keep_open: bool,
    ) -> mpsc::UnboundedReceiver<ClipEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let start = Instant::now();
            for (at, e) in events {
                tokio::time::sleep_until(start + secs(at)).await;
                let _ = tx.send(e);
            }
            if keep_open {
                std::future::pending::<()>().await;
            }
        });
        rx
    }

    /// A seek that settles `after` seconds from now.
    async fn seek_after(after: u64, ok: bool) -> Result<Duration, &'static str> {
        tokio::time::sleep(secs(after)).await;
        if ok {
            Ok(secs(83))
        } else {
            Err("removed")
        }
    }

    type NoSeek = std::future::Ready<Result<Duration, &'static str>>;

    #[tokio::test(start_paused = true)]
    async fn a_theme_is_audible_only_once_playable_within_20s() {
        use ClipEvent::*;
        let within = THEME_READY_WITHIN;
        let t = Instant::now();
        let mut rx = events_at(vec![(19, Playable)], true);
        assert_eq!(audible(None::<NoSeek>, &mut rx, within).await, Ok(()));
        assert_eq!(t.elapsed(), secs(19));

        let t = Instant::now();
        let mut rx = events_at(vec![(21, Playable)], true);
        assert_eq!(
            audible(None::<NoSeek>, &mut rx, within).await,
            Err(Unplayable::TimedOut)
        );
        assert_eq!(t.elapsed(), within, "gives up at 20 s");

        for dead in [vec![(1, Errored), (1, Ended)], vec![(1, Ended)]] {
            let mut rx = events_at(dead, true);
            assert_eq!(
                audible(None::<NoSeek>, &mut rx, within).await,
                Err(Unplayable::Failed)
            );
        }
        // Every sender gone (the track and its handlers dropped): failed.
        let mut rx = events_at(vec![], false);
        assert_eq!(
            audible(None::<NoSeek>, &mut rx, within).await,
            Err(Unplayable::Failed)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_seeked_theme_needs_the_seek_and_playable_inside_one_budget() {
        use ClipEvent::*;
        let within = THEME_READY_WITHIN;
        // Seek confirmed, then Playable: audible.
        let mut rx = events_at(vec![(2, Playable)], true);
        assert_eq!(
            audible(Some(seek_after(2, true)), &mut rx, within).await,
            Ok(())
        );
        // The stream opened (Playable) before the queued seek failed:
        // songbird removed the track, so it never played.
        let mut rx = events_at(vec![(1, Playable)], true);
        assert_eq!(
            audible(Some(seek_after(2, false)), &mut rx, within).await,
            Err(Unplayable::SeekFailed)
        );
        // A seek that hangs past the budget.
        let t = Instant::now();
        let mut rx = events_at(vec![], true);
        assert_eq!(
            audible(Some(seek_after(25, true)), &mut rx, within).await,
            Err(Unplayable::TimedOut)
        );
        assert_eq!(t.elapsed(), within);
        // One budget, not one per stage: a 15 s seek leaves 5 s.
        let t = Instant::now();
        let mut rx = events_at(vec![(21, Playable)], true);
        assert_eq!(
            audible(Some(seek_after(15, true)), &mut rx, within).await,
            Err(Unplayable::TimedOut)
        );
        assert_eq!(t.elapsed(), within);
    }

    /// A settle that takes `after` seconds, recording that it was started.
    async fn settle_after(
        after: u64,
        outcome: Settled,
        started: Arc<std::sync::atomic::AtomicBool>,
    ) -> Settled {
        started.store(true, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(secs(after)).await;
        outcome
    }

    /// Runs `run_clip` from now, and says when `stop` was called (from now).
    async fn clip_with(
        events: Vec<(u64, ClipEvent)>,
        millis_events: Vec<(u64, ClipEvent)>,
        settle: impl Future<Output = Settled>,
    ) -> (ClipOutcome, Option<Duration>, Duration) {
        let t = Instant::now();
        let mut rx = if millis_events.is_empty() {
            events_at(events, true)
        } else {
            let (tx, rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                for (at, e) in millis_events {
                    tokio::time::sleep_until(t + Duration::from_millis(at)).await;
                    let _ = tx.send(e);
                }
                std::future::pending::<()>().await;
            });
            rx
        };
        let stopped = std::sync::Mutex::new(None);
        let outcome = run_clip(&mut rx, t, settle, || {
            *stopped.lock().unwrap() = Some(t.elapsed());
        })
        .await;
        let at = *stopped.lock().unwrap();
        (outcome, at, t.elapsed())
    }

    fn flag() -> Arc<std::sync::atomic::AtomicBool> {
        Arc::new(std::sync::atomic::AtomicBool::new(false))
    }

    #[tokio::test(start_paused = true)]
    async fn a_clip_stops_at_10s_unless_it_ends_first() {
        use ClipEvent::*;
        // Ends by itself at 3 s: not stopped.
        let (out, stopped, took) = clip_with(
            vec![(3, Ended)],
            vec![],
            settle_after(0, Settled::Spent, flag()),
        )
        .await;
        assert_eq!(
            out,
            ClipOutcome::Played {
                ran_out: false,
                settled: Settled::Spent
            }
        );
        assert_eq!((stopped, took), (None, secs(3)));
        // A second Playable (after the seek) is not an end: stopped at 10 s.
        let (out, stopped, _) = clip_with(
            vec![(1, Playable)],
            vec![],
            settle_after(0, Settled::Spent, flag()),
        )
        .await;
        assert_eq!(
            out,
            ClipOutcome::Played {
                ran_out: true,
                settled: Settled::Spent
            }
        );
        assert_eq!(stopped, Some(THEME_CLIP));
        // The track gone (every sender dropped) is an end.
        let mut rx = events_at(vec![(4, Errored)], false);
        let out = run_clip(&mut rx, Instant::now(), async { Settled::Spent }, || {
            panic!("nothing to stop")
        })
        .await;
        assert!(matches!(out, ClipOutcome::Played { ran_out: false, .. }));
    }

    /// I1: the 10 s run from becoming audible; a slow settle (or a stalled
    /// card) never holds the stop.
    #[tokio::test(start_paused = true)]
    async fn a_slow_settle_never_holds_the_stop() {
        let (out, stopped, took) =
            clip_with(vec![], vec![], settle_after(5, Settled::Spent, flag())).await;
        assert_eq!(stopped, Some(THEME_CLIP), "5 s settle, stop at 10 s");
        assert_eq!(took, THEME_CLIP);
        assert_eq!(
            out,
            ClipOutcome::Played {
                ran_out: true,
                settled: Settled::Spent
            }
        );
        // Stalled past the clip: stopped on time, then the settle (started
        // once the first 500 ms passed) is waited out, never dropped
        // half-written.
        let (out, stopped, took) =
            clip_with(vec![], vec![], settle_after(15, Settled::Spent, flag())).await;
        assert_eq!(stopped, Some(THEME_CLIP));
        assert_eq!(took, THEME_TOO_SOON + secs(15));
        assert_eq!(
            out,
            ClipOutcome::Played {
                ran_out: true,
                settled: Settled::Spent
            }
        );
    }

    /// I2: refunded as it played (a cleanse, its end): stopped at once.
    #[tokio::test(start_paused = true)]
    async fn a_theme_refunded_as_it_plays_stops_at_once() {
        let (out, stopped, took) =
            clip_with(vec![], vec![], settle_after(2, Settled::Gone, flag())).await;
        assert_eq!(out, ClipOutcome::Refunded);
        // The settle starts once the first 500 ms passed.
        let at = THEME_TOO_SOON + secs(2);
        assert_eq!((stopped, took), (Some(at), at));
    }

    /// M6: an end within 500 ms of becoming audible (a seek that landed at
    /// the end) heard nothing: not settled, the caller refunds.
    #[tokio::test(start_paused = true)]
    async fn an_end_within_500ms_of_audible_is_unplayable() {
        use ClipEvent::*;
        let started = flag();
        let (out, stopped, _) = clip_with(
            vec![],
            vec![(300, Ended)],
            settle_after(0, Settled::Spent, started.clone()),
        )
        .await;
        assert_eq!(out, ClipOutcome::EndedAtOnce);
        assert_eq!(stopped, None);
        assert!(
            !started.load(std::sync::atomic::Ordering::SeqCst),
            "never settled"
        );
        // Past the 500 ms: it played.
        let started = flag();
        let (out, _, _) = clip_with(
            vec![],
            vec![(600, Ended)],
            settle_after(0, Settled::Spent, started.clone()),
        )
        .await;
        assert_eq!(
            out,
            ClipOutcome::Played {
                ran_out: false,
                settled: Settled::Spent
            }
        );
        assert!(started.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn theme_trigger_plays_on_a_join_or_the_move_out_of_the_hub() {
        const HUB: ChannelId = ChannelId::new(7);
        let hub = Some(HUB);
        // A join.
        assert_eq!(theme_trigger(None, Some(VC), hub), Some(VC));
        assert_eq!(theme_trigger(None, Some(VC), None), Some(VC), "no rooms");
        // A join into the hub: the move into their room follows.
        assert_eq!(theme_trigger(None, Some(HUB), hub), None);
        // The move out of the hub is the join.
        assert_eq!(theme_trigger(Some(HUB), Some(VC), hub), Some(VC));
        // An ordinary move, a stay, and a leave play nothing.
        assert_eq!(theme_trigger(Some(OTHER_VC), Some(VC), hub), None);
        assert_eq!(theme_trigger(Some(OTHER_VC), Some(VC), None), None);
        assert_eq!(theme_trigger(Some(VC), Some(VC), hub), None);
        assert_eq!(theme_trigger(Some(VC), None, hub), None);
        assert_eq!(theme_trigger(Some(HUB), None, hub), None, "left the hub");
        assert_eq!(theme_trigger(None, None, hub), None);
    }

    #[test]
    fn still_on_needs_an_active_row_and_the_target_still_there() {
        use ActionState::*;
        assert!(still_on(Some(Active), Some(VC), VC));
        assert!(!still_on(Some(Active), Some(OTHER_VC), VC), "moved");
        assert!(!still_on(Some(Active), None, VC), "left");
        for gone in [Ended, Done, Pending, Ending, Failed] {
            assert!(!still_on(Some(gone), Some(VC), VC), "{gone:?}");
        }
        assert!(!still_on(None, Some(VC), VC), "no row");
    }

    async fn rich(pool: &PgPool, user: UserId, credits: i64) {
        db::credit_earnings(
            pool,
            G,
            &[db::EarnRow {
                user,
                voice: credits,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    async fn state_of(pool: &PgPool, id: i64) -> (String, Option<String>) {
        sqlx::query_as("SELECT state, end_reason FROM economy_actions WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// A theme from A on `target`, bought at `now`, through the shop.
    async fn theme_on(data: &EconomyData, target: UserId, now: DateTime<Utc>) -> i64 {
        buy_theme(
            data,
            clean(),
            ThemeRequest {
                target,
                ..request(LINK, now)
            },
        )
        .await
        .unwrap()
        .action_id
    }

    db_test!(
        a_protected_target_gets_no_theme_and_the_buyer_is_refunded,
        |pool: PgPool| async {
            let data = themed_data(&pool).await;
            rich(&pool, A, 1000).await;
            // The real clock: the database compares ends with now().
            let now = Utc::now();
            let (immune, stored_out, memory_out) =
                (UserId::new(21), UserId::new(22), UserId::new(23));
            let free = theme_on(&data, T, now).await;
            // Two waiting for the immune one: both go back.
            let i1 = theme_on(&data, immune, now).await;
            let i2 = theme_on(&data, immune, now).await;
            let so = theme_on(&data, stored_out, now).await;
            let mo = theme_on(&data, memory_out, now).await;
            assert_eq!(db::balance(&pool, G, A).await.unwrap(), 500);

            db::grant_mod(&pool, G, immune, None, UserId::new(99), None)
                .await
                .unwrap();
            db::set_opted_out(&pool, G, stored_out, true, now)
                .await
                .unwrap();
            data.economy.set_one_opted_out(G, memory_out, true);

            let later = Utc::now();
            assert_eq!(claim_theme(&data, G, immune, later).await, None);
            assert_eq!(claim_theme(&data, G, stored_out, later).await, None);
            assert_eq!(claim_theme(&data, G, memory_out, later).await, None);
            for (id, why) in [
                (i1, "immunity"),
                (i2, "immunity"),
                (so, "opt_out"),
                (mo, "opt_out"),
            ] {
                assert_eq!(
                    state_of(&pool, id).await,
                    ("ended".to_string(), Some(why.to_string())),
                    "{id}"
                );
            }
            assert_eq!(db::balance(&pool, G, A).await.unwrap(), 900, "four back");
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_ledger WHERE kind = 'refund' AND delta = 100"
                )
                .await,
                4
            );
            // The unprotected target's theme is claimed, and left as it was.
            let row = claim_theme(&data, G, T, later).await.expect("claimed");
            assert_eq!(row.id, free);
            assert_eq!(state_of(&pool, free).await, ("active".to_string(), None));
            assert_eq!(db::balance(&pool, G, A).await.unwrap(), 900);
        }
    );

    db_test!(
        claim_theme_takes_the_oldest_and_leaves_the_rest,
        |pool: PgPool| async {
            let data = themed_data(&pool).await;
            rich(&pool, A, 1000).await;
            let now = Utc::now();
            // Already past its end (bought eight days ago): the sweeper's.
            let lapsed = theme_on(&data, T, now - Days::days(8)).await;
            let first = theme_on(&data, T, now).await;
            let second = theme_on(&data, T, now).await;
            assert_eq!(claim_theme(&data, G, UserId::new(77), now).await, None);
            let row = claim_theme(&data, G, T, Utc::now()).await.expect("claimed");
            assert_eq!(row.id, first, "the oldest still waiting");
            assert_eq!(
                row.payload,
                ActionPayload::Theme {
                    url: LINK.to_string(),
                    start_secs: 83
                }
            );
            // Nothing moved: the claim settles nothing.
            for id in [lapsed, first, second] {
                assert_eq!(state_of(&pool, id).await, ("active".to_string(), None));
            }
            // Once it has played, the next one is up.
            assert!(db::finish_action(&pool, first, ActionState::Done)
                .await
                .unwrap());
            let row = claim_theme(&data, G, T, Utc::now()).await.expect("next");
            assert_eq!(row.id, second);
        }
    );
}
