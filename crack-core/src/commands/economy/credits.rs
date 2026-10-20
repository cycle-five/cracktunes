//! `/credits balance|top|gift|history|optout|optin`.
//!
//! The pure `*_text` and `check_gift` functions carry the wording and the
//! rules; the poise functions are glue.
use crate::db::economy::{self as db, LedgerEntry, WalletSummary};
use crate::economy::gamble::{bet_refusal_text, BetRefusal};
use crate::economy::wallet::MoveError;
use crate::economy::{self, OptOutError};
use crate::messaging::courier;
use crate::messaging::messages::{
    ECONOMY_BALANCE_HAS, ECONOMY_GIFT_FAILED, ECONOMY_GIFT_FROM_OPTED_OUT,
    ECONOMY_GIFT_NON_POSITIVE, ECONOMY_GIFT_SELF, ECONOMY_GIFT_SENT, ECONOMY_GIFT_TO,
    ECONOMY_GIFT_TO_BOT, ECONOMY_GIFT_TO_OPTED_OUT, ECONOMY_HISTORY_EARNED,
    ECONOMY_HISTORY_FROM_MESSAGES, ECONOMY_HISTORY_IN_VOICE, ECONOMY_HISTORY_NO_ROWS,
    ECONOMY_HISTORY_TITLE, ECONOMY_LEDGER_ADJUSTMENT, ECONOMY_LEDGER_COINFLIP_BET,
    ECONOMY_LEDGER_COINFLIP_WIN, ECONOMY_LEDGER_GIFT_RECEIVED, ECONOMY_LEDGER_GIFT_SENT,
    ECONOMY_LEDGER_PURCHASE, ECONOMY_LEDGER_REFUND, ECONOMY_NO_DATABASE, ECONOMY_OFF,
    ECONOMY_OPTED_IN_DONE, ECONOMY_OPTED_OUT_DONE, ECONOMY_OPTOUT_ALREADY_IN,
    ECONOMY_OPTOUT_ALREADY_OUT, ECONOMY_OPTOUT_COOLDOWN_PREFIX, ECONOMY_OPTOUT_DISABLED,
    ECONOMY_OPTOUT_FAILED, ECONOMY_OPTOUT_SWITCH_BACK_PREFIX, ECONOMY_READ_FAILED,
    ECONOMY_TOP_EMPTY, ECONOMY_TOP_TITLE,
};
use crate::messaging::render::Rendered;
use crate::{Context, Error};
use chrono::{DateTime, Duration, Utc};
use serenity::all::{GuildId, User, UserId};
use sqlx::PgPool;
use std::collections::HashSet;

/// How many rows `/credits top` shows.
const LIST_LEN: i64 = 10;
/// Extra rows `/credits top` fetches so that hiding opted-out members still
/// leaves [`LIST_LEN`] to show.
const TOP_SLACK: i64 = 20;
/// How many ledger rows `/credits history` shows.
const HISTORY_LEN: i64 = 15;

/// Why a gift was refused before any credits moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftRefusal {
    SelfGift,
    ToBot,
    FromOptedOut,
    ToOptedOut,
    NonPositive,
}

/// Check a gift, refusing for the first reason that applies, in the enum's order.
pub fn check_gift(
    from: UserId,
    to: UserId,
    to_bot: bool,
    from_opted_out: bool,
    to_opted_out: bool,
    amount: i64,
) -> Result<(), GiftRefusal> {
    if from == to {
        return Err(GiftRefusal::SelfGift);
    }
    if to_bot {
        return Err(GiftRefusal::ToBot);
    }
    if from_opted_out {
        return Err(GiftRefusal::FromOptedOut);
    }
    if to_opted_out {
        return Err(GiftRefusal::ToOptedOut);
    }
    if amount <= 0 {
        return Err(GiftRefusal::NonPositive);
    }
    Ok(())
}

pub fn gift_refusal_text(r: GiftRefusal) -> &'static str {
    match r {
        GiftRefusal::SelfGift => ECONOMY_GIFT_SELF,
        GiftRefusal::ToBot => ECONOMY_GIFT_TO_BOT,
        GiftRefusal::FromOptedOut => ECONOMY_GIFT_FROM_OPTED_OUT,
        GiftRefusal::ToOptedOut => ECONOMY_GIFT_TO_OPTED_OUT,
        GiftRefusal::NonPositive => ECONOMY_GIFT_NON_POSITIVE,
    }
}

/// "<@1> has 40 🪙 credits." for a balance.
pub fn balance_text(user: UserId, balance: i64, currency: &str, emoji: &str) -> String {
    format!("<@{user}> {ECONOMY_BALANCE_HAS} {balance} {emoji} {currency}.")
}

/// A gift announced: who gave what to whom.
pub fn gift_text(from: UserId, to: UserId, amount: i64, currency: &str, emoji: &str) -> String {
    format!("<@{from}> {ECONOMY_GIFT_SENT} {amount} {emoji} {currency} {ECONOMY_GIFT_TO} <@{to}>.")
}

/// The ranking, richest first.
pub fn top_text(rows: &[(UserId, i64)], currency: &str, emoji: &str) -> String {
    if rows.is_empty() {
        return format!("**{ECONOMY_TOP_TITLE}**\n{ECONOMY_TOP_EMPTY}");
    }
    let mut lines = vec![format!("**{ECONOMY_TOP_TITLE}**")];
    lines.extend(
        rows.iter()
            .enumerate()
            .map(|(i, (user, bal))| format!("{}. <@{user}> {bal} {emoji} {currency}", i + 1)),
    );
    lines.join("\n")
}

/// The words for a stored ledger kind; a kind this build does not know shows
/// as stored.
pub fn ledger_label(kind: &str) -> &str {
    match kind {
        "purchase" => ECONOMY_LEDGER_PURCHASE,
        "refund" => ECONOMY_LEDGER_REFUND,
        "gift_sent" => ECONOMY_LEDGER_GIFT_SENT,
        "gift_received" => ECONOMY_LEDGER_GIFT_RECEIVED,
        "coinflip_stake" => ECONOMY_LEDGER_COINFLIP_BET,
        "coinflip_win" => ECONOMY_LEDGER_COINFLIP_WIN,
        "adjust" => ECONOMY_LEDGER_ADJUSTMENT,
        other => other,
    }
}

/// The member's activity: what they earned so far, then their ledger, newest
/// first, each change signed. Earnings are counters, so they are a line of
/// totals and never rows.
pub fn history_text(
    earned: &WalletSummary,
    rows: &[LedgerEntry],
    currency: &str,
    emoji: &str,
) -> String {
    let mut lines = vec![
        format!("**{ECONOMY_HISTORY_TITLE}**"),
        format!(
            "{ECONOMY_HISTORY_EARNED} {} {emoji} {currency} {ECONOMY_HISTORY_IN_VOICE}, {} {emoji} {currency} {ECONOMY_HISTORY_FROM_MESSAGES}.",
            earned.earned_voice, earned.earned_messages
        ),
    ];
    if rows.is_empty() {
        lines.push(ECONOMY_HISTORY_NO_ROWS.to_string());
        return lines.join("\n");
    }
    lines.extend(rows.iter().map(|e| {
        format!(
            "{:+} {emoji} {currency} · {} · <t:{}:R>",
            e.delta,
            ledger_label(&e.kind),
            e.created_at.timestamp()
        )
    }));
    lines.join("\n")
}

/// The top rows without the opted-out members, at most `len`.
pub fn visible_top(
    rows: Vec<(UserId, i64)>,
    opted_out: &HashSet<UserId>,
    len: usize,
) -> Vec<(UserId, i64)> {
    rows.into_iter()
        .filter(|(user, _)| !opted_out.contains(user))
        .take(len)
        .collect()
}

/// The words for an opt-out switch that changed nothing.
pub fn optout_error_text(err: OptOutError) -> String {
    match err {
        OptOutError::Disabled => ECONOMY_OPTOUT_DISABLED.to_string(),
        OptOutError::Cooldown(when) => {
            format!(
                "{ECONOMY_OPTOUT_COOLDOWN_PREFIX} <t:{}:R>.",
                when.timestamp()
            )
        },
        // `optout_result_text` words this per direction.
        OptOutError::AlreadyThere | OptOutError::Db => ECONOMY_OPTOUT_FAILED.to_string(),
        OptOutError::NoDatabase => ECONOMY_NO_DATABASE.to_string(),
    }
}

/// Opt-out/in result words, including the direction-specific "already".
///
/// `switch_back` is when the member may switch again; a success says so.
pub fn optout_result_text(
    out: bool,
    result: Result<(), OptOutError>,
    switch_back: Option<DateTime<Utc>>,
) -> String {
    let done = |text: &str| match switch_back {
        Some(when) => format!(
            "{text} {ECONOMY_OPTOUT_SWITCH_BACK_PREFIX} <t:{}:R>.",
            when.timestamp()
        ),
        None => text.to_string(),
    };
    match result {
        Ok(()) if out => done(ECONOMY_OPTED_OUT_DONE),
        Ok(()) => done(ECONOMY_OPTED_IN_DONE),
        Err(OptOutError::AlreadyThere) if out => ECONOMY_OPTOUT_ALREADY_OUT.to_string(),
        Err(OptOutError::AlreadyThere) => ECONOMY_OPTOUT_ALREADY_IN.to_string(),
        Err(err) => optout_error_text(err),
    }
}

#[cfg(not(tarpaulin_include))]
pub(super) async fn say(
    ctx: Context<'_>,
    text: impl Into<String>,
    private: bool,
) -> Result<(), Error> {
    courier::reply_rendered(ctx, Rendered::text(text), private).await?;
    Ok(())
}

/// The checks every economy command starts with: a server, the economy on,
/// a database. Replies privately and gives `None` when the economy is off or
/// there is no database. With no server (impossible: every command is
/// `guild_only`) it returns `None` silently.
#[cfg(not(tarpaulin_include))]
pub(super) async fn gate(ctx: Context<'_>) -> Result<Option<(GuildId, PgPool)>, Error> {
    let Some(guild) = ctx.guild_id() else {
        return Ok(None);
    };
    if !ctx.data().economy.is_enabled(guild) {
        say(ctx, ECONOMY_OFF, true).await?;
        return Ok(None);
    }
    let Some(pool) = ctx.data().database_pool.clone() else {
        say(ctx, ECONOMY_NO_DATABASE, true).await?;
        return Ok(None);
    };
    Ok(Some((guild, pool)))
}

/// Check your credits, the ranking, gifts, and opting out.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands("balance", "top", "gift", "history", "optout", "optin"),
    subcommand_required
)]
pub async fn credits(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show a member's credits.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn balance(
    ctx: Context<'_>,
    #[description = "Whose credits (default: yours)"] member: Option<User>,
) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let who = member.map(|u| u.id).unwrap_or(ctx.author().id);
    let cfg = ctx.data().economy.settings(guild).config;
    match db::balance(&pool, guild, who).await {
        Ok(bal) => {
            say(
                ctx,
                balance_text(who, bal, &cfg.currency_name, &cfg.currency_emoji),
                false,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: balance of {who} in {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}

/// Show the richest members.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn top(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let cfg = ctx.data().economy.settings(guild).config;
    match db::top(&pool, guild, LIST_LEN + TOP_SLACK).await {
        Ok(rows) => {
            let rows = visible_top(
                rows,
                &ctx.data().economy.opted_out(guild),
                LIST_LEN as usize,
            );
            say(
                ctx,
                top_text(&rows, &cfg.currency_name, &cfg.currency_emoji),
                false,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: top in {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}

/// Give some of your credits to another member.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn gift(
    ctx: Context<'_>,
    #[description = "Who gets them"] member: User,
    #[description = "How many"]
    #[min = 1]
    amount: i64,
) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let from = ctx.author().id;
    let to = member.id;
    let eco = &ctx.data().economy;
    if let Err(refusal) = check_gift(
        from,
        to,
        member.bot(),
        eco.is_opted_out(guild, from),
        eco.is_opted_out(guild, to),
        amount,
    ) {
        return say(ctx, gift_refusal_text(refusal), true).await;
    }
    let cfg = eco.settings(guild).config;
    match db::transfer(&pool, guild, from, to, amount).await {
        Ok(_) => {
            say(
                ctx,
                gift_text(from, to, amount, &cfg.currency_name, &cfg.currency_emoji),
                false,
            )
            .await
        },
        Err(MoveError::Insufficient { balance }) => {
            say(
                ctx,
                bet_refusal_text(
                    &BetRefusal::CantAfford { balance },
                    &cfg.currency_name,
                    &cfg.currency_emoji,
                ),
                true,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: gift {from} to {to} in {guild} failed: {err:?}");
            say(ctx, ECONOMY_GIFT_FAILED, true).await
        },
    }
}

/// Show your recent credit changes.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn history(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let cfg = ctx.data().economy.settings(guild).config;
    let user = ctx.author().id;
    let (earned, rows) = match tokio::try_join!(
        db::wallet_summary(&pool, guild, user),
        db::history(&pool, guild, user, HISTORY_LEN)
    ) {
        Ok(both) => both,
        Err(err) => {
            tracing::warn!("economy: history in {guild} failed: {err}");
            return say(ctx, ECONOMY_READ_FAILED, true).await;
        },
    };
    say(
        ctx,
        history_text(&earned, &rows, &cfg.currency_name, &cfg.currency_emoji),
        true,
    )
    .await
}

#[cfg(not(tarpaulin_include))]
async fn switch(ctx: Context<'_>, out: bool) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let now = Utc::now();
    let result = economy::opt_out(&ctx.data(), guild, ctx.author().id, out, now).await;
    let days = ctx
        .data()
        .economy
        .settings(guild)
        .config
        .optout_cooldown_days;
    let switch_back = (days > 0).then(|| now + Duration::days(i64::from(days)));
    say(ctx, optout_result_text(out, result, switch_back), true).await
}

/// Stop earning, buying, gambling and gifting here. Your credits are kept.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn optout(ctx: Context<'_>) -> Result<(), Error> {
    switch(ctx, true).await
}

/// Take part in the economy again.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn optin(ctx: Context<'_>) -> Result<(), Error> {
    switch(ctx, false).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn u(n: u64) -> UserId {
        UserId::new(n)
    }

    #[test]
    fn check_gift_order() {
        // Everything wrong at once: the first in order wins, then each falls away.
        assert_eq!(
            check_gift(u(1), u(1), true, true, true, 0),
            Err(GiftRefusal::SelfGift)
        );
        assert_eq!(
            check_gift(u(1), u(2), true, true, true, 0),
            Err(GiftRefusal::ToBot)
        );
        assert_eq!(
            check_gift(u(1), u(2), false, true, true, 0),
            Err(GiftRefusal::FromOptedOut)
        );
        assert_eq!(
            check_gift(u(1), u(2), false, false, true, 0),
            Err(GiftRefusal::ToOptedOut)
        );
        assert_eq!(
            check_gift(u(1), u(2), false, false, false, 0),
            Err(GiftRefusal::NonPositive)
        );
        assert_eq!(
            check_gift(u(1), u(2), false, false, false, -5),
            Err(GiftRefusal::NonPositive)
        );
        assert_eq!(check_gift(u(1), u(2), false, false, false, 1), Ok(()));
    }

    #[test]
    fn balance_and_gift_text() {
        assert_eq!(
            balance_text(u(7), 40, "credits", "🪙"),
            "<@7> has 40 🪙 credits."
        );
        assert_eq!(
            gift_text(u(1), u(2), 25, "credits", "🪙"),
            "<@1> gifted 25 🪙 credits to <@2>."
        );
    }

    #[test]
    fn top_text_ranks_and_formats() {
        assert_eq!(
            top_text(&[(u(3), 900), (u(1), 40)], "gold", "🥇"),
            "**Richest members**\n1. <@3> 900 🥇 gold\n2. <@1> 40 🥇 gold"
        );
        assert_eq!(
            top_text(&[], "gold", "🥇"),
            "**Richest members**\nNobody has any yet."
        );
    }

    fn earned(voice: i64, messages: i64) -> WalletSummary {
        WalletSummary {
            balance: 0,
            earned_voice: voice,
            earned_messages: messages,
        }
    }

    #[test]
    fn history_text_signs_amounts_and_leads_with_earnings() {
        let at = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let rows = [
            LedgerEntry {
                delta: 100,
                kind: "coinflip_win".into(),
                created_at: at,
            },
            LedgerEntry {
                delta: -50,
                kind: "gift_sent".into(),
                created_at: at,
            },
        ];
        assert_eq!(
            history_text(&earned(30, 12), &rows, "credits", "🪙"),
            "**Your recent activity**\n\
             Earned so far: 30 🪙 credits in voice, 12 🪙 credits from messages.\n\
             +100 🪙 credits · coinflip win · <t:1700000000:R>\n\
             -50 🪙 credits · gift sent · <t:1700000000:R>"
        );
    }

    #[test]
    fn history_with_no_rows_still_shows_earnings() {
        assert_eq!(
            history_text(&earned(5, 0), &[], "credits", "🪙"),
            "**Your recent activity**\n\
             Earned so far: 5 🪙 credits in voice, 0 🪙 credits from messages.\n\
             No purchases, gifts or bets yet."
        );
    }

    #[test]
    fn ledger_labels_are_named_and_unknown_kinds_show_raw() {
        for (kind, label) in [
            ("purchase", "purchase"),
            ("refund", "refund"),
            ("gift_sent", "gift sent"),
            ("gift_received", "gift received"),
            ("coinflip_stake", "coinflip bet"),
            ("coinflip_win", "coinflip win"),
            ("adjust", "adjustment"),
        ] {
            assert_eq!(ledger_label(kind), label, "{kind}");
        }
        assert_eq!(ledger_label("prank_refund"), "prank_refund");
    }

    #[test]
    fn every_ledger_kind_has_a_label() {
        use crate::economy::wallet::LedgerKind::*;
        let known = [
            ECONOMY_LEDGER_PURCHASE,
            ECONOMY_LEDGER_REFUND,
            ECONOMY_LEDGER_GIFT_SENT,
            ECONOMY_LEDGER_GIFT_RECEIVED,
            ECONOMY_LEDGER_COINFLIP_BET,
            ECONOMY_LEDGER_COINFLIP_WIN,
            ECONOMY_LEDGER_ADJUSTMENT,
        ];
        let labels: HashSet<&str> = [
            Purchase,
            Refund,
            GiftSent,
            GiftReceived,
            CoinflipStake,
            CoinflipWin,
            Adjust,
        ]
        .into_iter()
        .map(|k| ledger_label(k.as_str()))
        .collect();
        assert_eq!(labels, HashSet::from(known), "each kind has its own label");
    }

    #[test]
    fn visible_top_hides_the_opted_out_and_still_fills_the_list() {
        let rows: Vec<(UserId, i64)> = (1..=5).map(|n| (u(n), 100 - n as i64)).collect();
        let out = HashSet::from([u(1), u(3)]);
        assert_eq!(
            visible_top(rows.clone(), &out, 2),
            vec![(u(2), 98), (u(4), 96)]
        );
        assert_eq!(visible_top(rows, &out, 10).len(), 3);
    }

    #[test]
    fn optout_words_by_direction() {
        let at = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(
            optout_result_text(true, Err(OptOutError::AlreadyThere), None),
            "You've already opted out."
        );
        assert_eq!(
            optout_result_text(false, Err(OptOutError::AlreadyThere), None),
            "You're already opted in."
        );
        assert_eq!(
            optout_result_text(true, Err(OptOutError::Cooldown(at)), None),
            "You switched recently. You can switch again <t:1700000000:R>."
        );
        assert_eq!(
            optout_result_text(false, Ok(()), None),
            "You've opted back in."
        );
        assert_eq!(
            optout_result_text(true, Err(OptOutError::Disabled), None),
            "This server doesn't allow opting out."
        );
    }

    #[test]
    fn a_successful_switch_states_when_you_can_switch_back() {
        let at = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(
            optout_result_text(false, Ok(()), Some(at)),
            "You've opted back in. You can switch back <t:1700000000:R>."
        );
        assert!(optout_result_text(true, Ok(()), Some(at))
            .ends_with(" You can switch back <t:1700000000:R>."));
        // A refusal does not mention it, and a zero cooldown omits it.
        assert_eq!(
            optout_result_text(true, Err(OptOutError::AlreadyThere), Some(at)),
            "You've already opted out."
        );
        assert_eq!(
            optout_result_text(true, Ok(()), None),
            ECONOMY_OPTED_OUT_DONE
        );
    }
}
