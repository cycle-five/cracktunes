//! `/wallet show|link|unlink|withdraw`: turning credits into CrackCoin on devnet. The rules and the
//! wording are in [`crate::coin::checks`]; the money moves in [`crate::coin::flow`]. Every reply
//! is private, and an address is only ever shown shortened.

use super::credits::{gate_for, say, GateNeed};
use crate::coin::checks::{
    address_refusal_text, check_address, check_withdraw_chain, check_withdraw_local, minting_text,
    outcome_text, readiness, refusal_text, short_address, show_text, WithdrawRefusal,
};
use crate::coin::flow::{self, Timing, WithdrawRequest};
use crate::coin::registry::CoinState;
use crate::db::economy::coin::{
    link_address, linked_address, sponsored_accounts, unlink_address, wallet_overview, StartError,
};
use crate::economy::gamble::{bet_refusal_text, BetRefusal};
use crate::economy::wallet::MoveError;
use crate::messaging::courier;
use crate::messaging::messages::{
    COIN_ALREADY_PENDING, COIN_FAILED, COIN_LINKED, COIN_NOT_LINKED, COIN_UNLINKED,
    ECONOMY_READ_FAILED,
};
use crate::messaging::render::Rendered;
use crate::{Context, Error};

/// Turn credits into CrackCoin, a worthless devnet token.
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands("show", "link", "unlink", "withdraw"),
    subcommand_required
)]
pub async fn wallet(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// The mint, once CrackCoin is ready.
fn ready_mint(ctx: Context<'_>) -> Option<String> {
    match ctx.data().economy.coin.state() {
        CoinState::Ready(chain) => Some(chain.mint().to_owned()),
        _ => None,
    }
}

/// Show your linked address and what you have withdrawn.
#[poise::command(slash_command, guild_only)]
pub async fn show(ctx: Context<'_>) -> Result<(), Error> {
    let Some((_guild, pool)) = gate_for(ctx, GateNeed::DatabaseOnly).await? else {
        return Ok(());
    };
    let user = ctx.author().id;
    let (address, overview) =
        match tokio::try_join!(linked_address(&pool, user), wallet_overview(&pool, user)) {
            Ok(both) => both,
            Err(err) => {
                tracing::warn!("crack-coin: /wallet show for {user} failed: {err:?}");
                return say(ctx, ECONOMY_READ_FAILED, true).await;
            },
        };
    let mint = ready_mint(ctx);
    say(
        ctx,
        show_text(address.as_deref(), mint.as_deref(), &overview),
        true,
    )
    .await
}

/// Link the devnet address CrackCoin is sent to.
#[poise::command(slash_command, guild_only)]
pub async fn link(
    ctx: Context<'_>,
    #[description = "A Solana devnet wallet address"] address: String,
) -> Result<(), Error> {
    let Some((_guild, pool)) = gate_for(ctx, GateNeed::DatabaseOnly).await? else {
        return Ok(());
    };
    let mint = ready_mint(ctx);
    let canonical = match check_address(&address, mint.as_deref()) {
        Ok(a) => a,
        Err(refusal) => return say(ctx, address_refusal_text(&refusal), true).await,
    };
    let user = ctx.author().id;
    match link_address(&pool, user, &canonical).await {
        Ok(()) => {
            say(
                ctx,
                format!("{COIN_LINKED}\n`{}`", short_address(&canonical)),
                true,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("crack-coin: /wallet link for {user} failed: {err:?}");
            say(ctx, COIN_FAILED, true).await
        },
    }
}

/// Forget your linked address.
#[poise::command(slash_command, guild_only)]
pub async fn unlink(ctx: Context<'_>) -> Result<(), Error> {
    let Some((_guild, pool)) = gate_for(ctx, GateNeed::DatabaseOnly).await? else {
        return Ok(());
    };
    let user = ctx.author().id;
    match unlink_address(&pool, user).await {
        Ok(true) => say(ctx, COIN_UNLINKED, true).await,
        Ok(false) => say(ctx, COIN_NOT_LINKED, true).await,
        Err(err) => {
            tracing::warn!("crack-coin: /wallet unlink for {user} failed: {err:?}");
            say(ctx, COIN_FAILED, true).await
        },
    }
}

/// Spend credits to mint CrackCoin to your linked address.
#[poise::command(slash_command, guild_only)]
pub async fn withdraw(
    ctx: Context<'_>,
    #[description = "How many credits to turn into CrackCoin"]
    #[min = 1]
    amount: i64,
) -> Result<(), Error> {
    let Some((guild, pool)) = gate_for(ctx, GateNeed::EconomyOn).await? else {
        return Ok(());
    };
    // The chain reads below can outlast Discord's 3 seconds.
    ctx.defer_ephemeral().await?;
    let user = ctx.author().id;
    let data = ctx.data();
    let eco = &data.economy;
    let state = eco.coin.state();
    let address = match linked_address(&pool, user).await {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!("crack-coin: withdraw: reading the link of {user} failed: {err:?}");
            return say(ctx, ECONOMY_READ_FAILED, true).await;
        },
    };
    let cfg = eco.settings(guild).config;
    if let Err(refusal) = check_withdraw_local(
        readiness(&state),
        cfg.coin_withdrawals,
        eco.is_opted_out(guild, user),
        address.is_some(),
        amount,
    ) {
        return say(ctx, refusal_text(&refusal), true).await;
    }
    // Both are certain after the local checks; stay defensive rather than unwrap.
    let (CoinState::Ready(chain), Some(address)) = (state, address) else {
        return say(ctx, COIN_FAILED, true).await;
    };
    // The link may predate the mint (or name it): re-check before any read or charge.
    if let Err(refusal) = check_address(&address, Some(chain.mint())) {
        return say(ctx, address_refusal_text(&refusal), true).await;
    }
    // Every refusal comes before the debit, and a read that fails refuses too.
    let reads = tokio::try_join!(
        async { chain.tap_lamports().await.map_err(|e| e.to_string()) },
        async {
            chain
                .has_token_account(&address)
                .await
                .map_err(|e| e.to_string())
        },
        async {
            sponsored_accounts(&pool, user, chain.mint())
                .await
                .map_err(|e| e.to_string())
        },
    );
    let (tap, has_account, used) = match reads {
        Ok(reads) => reads,
        Err(why) => {
            tracing::warn!("crack-coin: withdraw pre-check for {user} failed: {why}");
            return say(ctx, COIN_FAILED, true).await;
        },
    };
    let sponsored = match check_withdraw_chain(tap, has_account, used) {
        Ok(sponsored) => sponsored,
        Err(refusal) => {
            if refusal == WithdrawRefusal::TapDry {
                tracing::warn!("crack-coin: the tap is dry ({tap} lamports)");
            }
            return say(ctx, refusal_text(&refusal), true).await;
        },
    };
    // One request, passed to both steps: the mint goes where the charge was snapshotted.
    let req = WithdrawRequest {
        guild,
        user,
        address: &address,
        amount,
        sponsored,
    };
    let started = match flow::begin(&pool, chain.as_ref(), &req).await {
        Ok(started) => started,
        Err(StartError::Money(MoveError::Insufficient { balance })) => {
            return say(
                ctx,
                bet_refusal_text(
                    &BetRefusal::CantAfford { balance },
                    &cfg.currency_name,
                    &cfg.currency_emoji,
                ),
                true,
            )
            .await;
        },
        Err(StartError::AlreadyPending) => return say(ctx, COIN_ALREADY_PENDING, true).await,
        Err(err) => {
            tracing::warn!(
                "crack-coin: withdraw of {amount} by {user} in {guild} not started: {err:?}"
            );
            return say(ctx, COIN_FAILED, true).await;
        },
    };
    // From here the member has been charged: every path reaches `complete` with the same `req`.
    // A failed placeholder is logged, never propagated.
    let handle =
        match courier::reply_rendered(ctx, Rendered::text(minting_text(amount, &address)), true)
            .await
        {
            Ok(handle) => Some(handle),
            Err(err) => {
                tracing::warn!(
                    "crack-coin: withdrawal {}: placeholder not sent: {err:?}",
                    started.id
                );
                None
            },
        };
    let outcome = flow::complete(&pool, chain.as_ref(), &started, &req, &Timing::LIVE).await;
    let text = outcome_text(&outcome, amount, &address, &cfg.currency_name);
    // A failed reply must not reach `on_error`, which answers publicly.
    let sent = match &handle {
        Some(handle) => courier::edit_rendered(ctx, handle, Rendered::text(text.clone()))
            .await
            .is_ok(),
        None => false,
    };
    if !sent {
        if let Err(err) = say(ctx, text, true).await {
            tracing::warn!(
                "crack-coin: withdrawal {}: result not sent: {err:?}",
                started.id
            );
        }
    }
    Ok(())
}
