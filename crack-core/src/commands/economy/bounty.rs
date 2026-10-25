//! `/bounty place|list`: put credits on a member's head, and see what stands.
use super::admin::log_to;
use super::credits::{gate, say};
use super::shop::note_error_text;
use crate::db::economy as db;
use crate::economy::bounty::{
    bounty_card_channel, bounty_card_text, bounty_list_text, place as place_bounty, BountyRequest,
};
use crate::economy::settings::{validate_note, ActionKind};
use crate::economy::shop::buy_error_text;
use crate::economy::target::{gather_facts, Subject};
use crate::messaging::courier;
use crate::messaging::messages::{
    ECONOMY_BOUNTY_PLACED, ECONOMY_CANT_TELL, ECONOMY_READ_FAILED, ECONOMY_SHOP_BOT_BUYER,
    ECONOMY_YOU_HAVE,
};
use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::modlog::event::ModLogEvent;
use crate::{Context, Error};
use chrono::Utc;
use crack_types::discord::ToRef;
use serenity::all::{ChannelId, GenericChannelId, Member};

/// Put credits on a member's head, or see the bounties that stand.
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    subcommands("place", "list"),
    subcommand_required
)]
pub async fn bounty(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// The private reply to a placed bounty.
pub fn placed_reply_text(balance: i64, currency: &str, emoji: &str) -> String {
    format!("{ECONOMY_BOUNTY_PLACED} {ECONOMY_YOU_HAVE} {balance} {emoji} {currency}.")
}

/// Put credits on a member's head. Whoever next lands a prank on them collects.
#[poise::command(slash_command, guild_only)]
// live-Discord glue: a command over `Context`; only its mod-log event's ids became refs here, and the event's wording is pinned by `modlog_events_render_as_before`.
#[cfg_attr(test, mutants::skip)]
pub async fn place(
    ctx: Context<'_>,
    #[description = "Whose head"] member: Member,
    #[description = "How many credits to put up"] amount: i64,
    #[description = "A note shown on the card"] note: Option<String>,
    #[description = "Put your name on the card"] signed: Option<bool>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    // Reads the database and Discord before the first reply, as a prank does.
    ctx.defer_ephemeral().await?;
    let Some(placer) = ctx.author_member().await else {
        return say(ctx, ECONOMY_CANT_TELL, true).await;
    };
    let data = ctx.data();
    let cfg = data.economy.settings(guild).config;
    if placer.user.bot() {
        return say(ctx, ECONOMY_SHOP_BOT_BUYER, true).await;
    }
    let note = match note.as_deref().map(validate_note).transpose() {
        Ok(n) => n,
        Err(e) => return say(ctx, note_error_text(e), true).await,
    };
    let signed = signed.unwrap_or(false);
    let now = Utc::now();
    let (placer_id, target) = (placer.user.id, member.user.id);
    let facts = match gather_facts(
        &data,
        &ctx.serenity_context().cache,
        guild,
        Subject {
            member: &member,
            is_self: target == placer_id,
        },
        ActionKind::Bounty,
        None,
        now,
    )
    .await
    {
        Ok(f) => f,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    let placed = match place_bounty(
        &data,
        facts,
        BountyRequest {
            guild,
            placer: placer_id,
            target,
            amount,
            note: note.clone(),
            signed,
            now,
        },
    )
    .await
    {
        Ok(p) => p,
        Err(e) => return say(ctx, buy_error_text(&e, &cfg.currency_name), true).await,
    };
    let replied = say(
        ctx,
        placed_reply_text(placed.balance, &cfg.currency_name, &cfg.currency_emoji),
        true,
    )
    .await;
    let channel = bounty_card_channel(
        ChannelId::new(ctx.channel_id().get()),
        cfg.announce_channel_id,
    );
    let rendered = Rendered::text(bounty_card_text(
        amount,
        &cfg.currency_name,
        target,
        placed.until,
        note.as_deref(),
        signed.then_some(placer_id),
    ));
    let transport = DiscordTransport::of(ctx.serenity_context());
    if let Err(err) =
        courier::post_message(&transport, GenericChannelId::new(channel.get()), &rendered).await
    {
        // The channel may be one the bot cannot post in: a public follow-up in
        // the invoking channel needs no channel permissions.
        tracing::warn!("economy: posting the bounty card in {channel} failed: {err:?}");
        if let Err(err) = courier::reply_rendered(ctx, rendered, false).await {
            tracing::warn!("economy: the bounty card follow-up failed: {err:?}");
        }
    }
    log_to(
        &data,
        ctx.serenity_context(),
        guild,
        ModLogEvent::ShopAction {
            action: ActionKind::Bounty,
            buyer: placer_id.to_ref(),
            target: target.to_ref(),
            note,
            signed,
            until: Some(placed.until),
            mirrored_from: None,
        },
    )
    .await;
    replied
}

/// See who has a bounty on their head.
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let cfg = ctx.data().economy.settings(guild).config;
    match db::live_bounty_stakes(&pool, guild).await {
        Ok(stakes) => {
            say(
                ctx,
                bounty_list_text(&stakes, &cfg.currency_name, &cfg.currency_emoji),
                true,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: listing the bounties in {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}
