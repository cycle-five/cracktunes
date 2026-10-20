//! `/credits balance|top|gift|history|optout|optin`.
//!
//! The pure `*_text` and `check_gift` functions carry the wording and the
//! rules; the poise functions are glue.
use crate::db::economy::{self as db, LedgerEntry};
use crate::economy::gamble::{bet_refusal_text, BetRefusal};
use crate::economy::wallet::MoveError;
use crate::economy::{self, OptOutError};
use crate::messaging::courier;
use crate::messaging::messages::{
    ECONOMY_BALANCE_HAS, ECONOMY_GIFT_FAILED, ECONOMY_GIFT_FROM_OPTED_OUT,
    ECONOMY_GIFT_NON_POSITIVE, ECONOMY_GIFT_SELF, ECONOMY_GIFT_SENT, ECONOMY_GIFT_TO,
    ECONOMY_GIFT_TO_BOT, ECONOMY_GIFT_TO_OPTED_OUT, ECONOMY_HISTORY_EMPTY, ECONOMY_HISTORY_TITLE,
    ECONOMY_NO_DATABASE, ECONOMY_OFF, ECONOMY_OPTED_IN_DONE, ECONOMY_OPTED_OUT_DONE,
    ECONOMY_OPTOUT_ALREADY_IN, ECONOMY_OPTOUT_ALREADY_OUT, ECONOMY_OPTOUT_COOLDOWN_PREFIX,
    ECONOMY_OPTOUT_DISABLED, ECONOMY_OPTOUT_FAILED, ECONOMY_READ_FAILED, ECONOMY_TOP_EMPTY,
    ECONOMY_TOP_TITLE,
};
use crate::messaging::render::Rendered;
use crate::{Context, Error};
use chrono::Utc;
use serenity::all::{GuildId, User, UserId};
use sqlx::PgPool;

/// How many rows `/credits top` shows.
const LIST_LEN: i64 = 10;
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

/// The member's ledger, newest first, each change signed.
pub fn history_text(rows: &[LedgerEntry], currency: &str, emoji: &str) -> String {
    if rows.is_empty() {
        return format!("**{ECONOMY_HISTORY_TITLE}**\n{ECONOMY_HISTORY_EMPTY}");
    }
    let mut lines = vec![format!("**{ECONOMY_HISTORY_TITLE}**")];
    lines.extend(rows.iter().map(|e| {
        format!(
            "{:+} {emoji} {currency} · {} · <t:{}:R>",
            e.delta,
            e.kind.replace('_', " "),
            e.created_at.timestamp()
        )
    }));
    lines.join("\n")
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
pub fn optout_result_text(out: bool, result: Result<(), OptOutError>) -> String {
    match result {
        Ok(()) if out => ECONOMY_OPTED_OUT_DONE.to_string(),
        Ok(()) => ECONOMY_OPTED_IN_DONE.to_string(),
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
    match db::top(&pool, guild, LIST_LEN).await {
        Ok(rows) => {
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
    #[description = "How many"] amount: i64,
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
    match db::history(&pool, guild, ctx.author().id, HISTORY_LEN).await {
        Ok(rows) => {
            say(
                ctx,
                history_text(&rows, &cfg.currency_name, &cfg.currency_emoji),
                true,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: history in {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}

#[cfg(not(tarpaulin_include))]
async fn switch(ctx: Context<'_>, out: bool) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let result = economy::opt_out(&ctx.data(), guild, ctx.author().id, out, Utc::now()).await;
    say(ctx, optout_result_text(out, result), true).await
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

    #[test]
    fn history_text_signs_amounts() {
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
            history_text(&rows, "credits", "🪙"),
            "**Your recent activity**\n\
             +100 🪙 credits · coinflip win · <t:1700000000:R>\n\
             -50 🪙 credits · gift sent · <t:1700000000:R>"
        );
        assert_eq!(
            history_text(&[], "credits", "🪙"),
            "**Your recent activity**\nNothing yet."
        );
    }

    #[test]
    fn optout_words_by_direction() {
        let at = chrono::Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(
            optout_result_text(true, Err(OptOutError::AlreadyThere)),
            "You've already opted out."
        );
        assert_eq!(
            optout_result_text(false, Err(OptOutError::AlreadyThere)),
            "You're already opted in."
        );
        assert_eq!(
            optout_result_text(true, Err(OptOutError::Cooldown(at))),
            "You switched recently. You can switch again <t:1700000000:R>."
        );
        assert_eq!(optout_result_text(false, Ok(())), "You've opted back in.");
        assert_eq!(
            optout_result_text(true, Err(OptOutError::Disabled)),
            "This server doesn't allow opting out."
        );
    }
}
