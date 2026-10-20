//! `/economy status|grant|lift|adjust`, for moderators.
use super::credits::{gate, say};
use crate::db::economy as db;
use crate::economy::settings::EconomySettings;
use crate::economy::wallet::MoveError;
use crate::economy::{mod_grant, mod_lift, GrantError};
use crate::messaging::messages::{
    ECONOMY_ADMIN_ADJUST_BELOW_ZERO_PREFIX, ECONOMY_ADMIN_ADJUST_DONE, ECONOMY_ADMIN_ADJUST_FAILED,
    ECONOMY_ADMIN_ADJUST_NEEDS_MANAGE, ECONOMY_ADMIN_ADJUST_TOO_BIG, ECONOMY_ADMIN_ADJUST_ZERO,
    ECONOMY_ADMIN_BOT_TARGET, ECONOMY_ADMIN_GRANT_DISABLED, ECONOMY_ADMIN_GRANT_DONE,
    ECONOMY_ADMIN_GRANT_FAILED, ECONOMY_ADMIN_HOURS_RANGE, ECONOMY_ADMIN_LIFT_DONE,
    ECONOMY_ADMIN_LIFT_FAILED, ECONOMY_ADMIN_LIFT_NONE, ECONOMY_ADMIN_NEEDS_MODERATE,
    ECONOMY_NO_DATABASE, ECONOMY_READ_FAILED, ECONOMY_STATUS_CURRENCY, ECONOMY_STATUS_EARNING,
    ECONOMY_STATUS_GAMBLING, ECONOMY_STATUS_HELD, ECONOMY_STATUS_IMMUNITY, ECONOMY_STATUS_MAX_BET,
    ECONOMY_STATUS_MODS, ECONOMY_STATUS_OFF, ECONOMY_STATUS_ON, ECONOMY_STATUS_PER_MESSAGE,
    ECONOMY_STATUS_PER_VOICE_MINUTE, ECONOMY_STATUS_SECONDS, ECONOMY_STATUS_SHOP,
    ECONOMY_STATUS_TITLE, ECONOMY_STATUS_WALLETS,
};
use crate::messaging::transport::DiscordTransport;
use crate::modlog::batch::ModLogSink;
use crate::modlog::event::ModLogEvent;
use crate::modlog::live::ChannelSink;
use crate::{Context, Error};
use chrono::{Duration, Utc};
use serenity::all::{GuildId, Permissions, User};
use std::sync::Arc;

/// Longest immunity a moderator can grant by hours: a year.
pub const MAX_GRANT_HOURS: u32 = 8760;

/// Largest single adjustment, up or down.
pub const MAX_ADJUST: i64 = 1_000_000_000;

fn on_off(b: bool) -> &'static str {
    if b {
        ECONOMY_STATUS_ON
    } else {
        ECONOMY_STATUS_OFF
    }
}

/// The server's economy at a glance.
pub fn economy_status_text(s: &EconomySettings, wallets: i64, credits: i64) -> String {
    let c = &s.config;
    let (emoji, name) = (&c.currency_emoji, &c.currency_name);
    format!(
        "**{ECONOMY_STATUS_TITLE}**: {}\n\
         {ECONOMY_STATUS_CURRENCY}: {emoji} {name}\n\
         {ECONOMY_STATUS_WALLETS}: {wallets}\n\
         {ECONOMY_STATUS_HELD}: {credits} {emoji} {name}\n\
         {ECONOMY_STATUS_EARNING}: {} {ECONOMY_STATUS_PER_VOICE_MINUTE}, {} {ECONOMY_STATUS_PER_MESSAGE} {}{ECONOMY_STATUS_SECONDS}\n\
         {ECONOMY_STATUS_IMMUNITY} {}, {ECONOMY_STATUS_SHOP} {}, {ECONOMY_STATUS_MODS} {}\n\
         {ECONOMY_STATUS_GAMBLING}: {}, {ECONOMY_STATUS_MAX_BET} {}",
        on_off(s.enabled),
        c.voice_credits_per_minute,
        c.message_credits,
        c.message_cooldown_seconds,
        on_off(c.immunity_optout),
        on_off(c.immunity_shop),
        on_off(c.immunity_mods),
        on_off(c.gambling_enabled),
        c.max_bet,
    )
}

/// Moderator commands need Moderate Members (an administrator holds every
/// permission, so the resolved set already includes it; checked anyway).
pub fn mod_allowed(p: Permissions) -> bool {
    p.contains(Permissions::MODERATE_MEMBERS) || p.contains(Permissions::ADMINISTRATOR)
}

/// Whether the hours of a grant are in range.
pub fn hours_ok(h: u32) -> bool {
    (1..=MAX_GRANT_HOURS).contains(&h)
}

/// The invoker's permissions from the interaction (the resolved set); none
/// when unknown.
#[cfg(not(tarpaulin_include))]
async fn invoker_permissions(ctx: Context<'_>) -> Option<Permissions> {
    ctx.author_member().await.and_then(|m| m.permissions)
}

/// Refuses privately unless the invoker has Moderate Members.
#[cfg(not(tarpaulin_include))]
async fn mod_gate(ctx: Context<'_>) -> Result<bool, Error> {
    if invoker_permissions(ctx).await.is_some_and(mod_allowed) {
        return Ok(true);
    }
    say(ctx, ECONOMY_ADMIN_NEEDS_MODERATE, true).await?;
    Ok(false)
}

/// Adjusting balances needs Manage Server (or Administrator, which holds every
/// permission), on top of the command's own Moderate Members.
pub fn adjust_allowed(p: Permissions) -> bool {
    p.contains(Permissions::MANAGE_GUILD) || p.contains(Permissions::ADMINISTRATOR)
}

/// Whether an adjustment is within [`MAX_ADJUST`] either way.
pub fn adjust_amount_ok(amount: i64) -> bool {
    amount.unsigned_abs() <= MAX_ADJUST.unsigned_abs()
}

/// Post a mod log event; nothing happens while the log is off.
#[cfg(not(tarpaulin_include))]
async fn log(ctx: Context<'_>, guild: GuildId, event: ModLogEvent) {
    let sink = ChannelSink {
        data: (*ctx.data()).clone(),
        transport: Arc::new(DiscordTransport::of(ctx.serenity_context())),
    };
    sink.post(guild, event).await;
}

/// Mod tools for the economy.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Economy",
    slash_command,
    guild_only,
    default_member_permissions = "MODERATE_MEMBERS",
    subcommands("status", "grant", "lift", "adjust"),
    subcommand_required
)]
pub async fn economy(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show how the economy is set up here.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    if !mod_gate(ctx).await? {
        return Ok(());
    }
    match db::stats(&pool, guild).await {
        Ok((wallets, credits)) => {
            let s = ctx.data().economy.settings(guild);
            say(ctx, economy_status_text(&s, wallets, credits), true).await
        },
        Err(err) => {
            tracing::warn!("economy: stats of {guild} failed: {err}");
            say(ctx, ECONOMY_READ_FAILED, true).await
        },
    }
}

/// Make a member immune to pranks, for some hours or until lifted.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn grant(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "Hours (1 to 8760); leave out for until lifted"] hours: Option<u32>,
    #[description = "Why"] reason: Option<String>,
) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    if !mod_gate(ctx).await? {
        return Ok(());
    }
    if member.bot() {
        return say(ctx, ECONOMY_ADMIN_BOT_TARGET, true).await;
    }
    if hours.is_some_and(|h| !hours_ok(h)) {
        return say(ctx, ECONOMY_ADMIN_HOURS_RANGE, true).await;
    }
    let until = hours.map(|h| Utc::now() + Duration::hours(i64::from(h)));
    let by = ctx.author().id;
    match mod_grant(&ctx.data(), guild, member.id, until, by, reason.as_deref()).await {
        Ok(()) => {
            // Reply first: a slow log channel must not time the interaction out.
            let replied = say(ctx, ECONOMY_ADMIN_GRANT_DONE, true).await;
            log(
                ctx,
                guild,
                ModLogEvent::ImmunityGranted {
                    by,
                    target: member.id,
                    until,
                    reason,
                },
            )
            .await;
            replied
        },
        Err(GrantError::Disabled) => say(ctx, ECONOMY_ADMIN_GRANT_DISABLED, true).await,
        Err(GrantError::NoDatabase) => say(ctx, ECONOMY_NO_DATABASE, true).await,
        Err(GrantError::Db) => say(ctx, ECONOMY_ADMIN_GRANT_FAILED, true).await,
    }
}

/// Remove a member's moderator immunity.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn lift(ctx: Context<'_>, #[description = "Who"] member: User) -> Result<(), Error> {
    let Some((guild, _pool)) = gate(ctx).await? else {
        return Ok(());
    };
    if !mod_gate(ctx).await? {
        return Ok(());
    }
    match mod_lift(&ctx.data(), guild, member.id).await {
        Ok(true) => {
            let replied = say(ctx, ECONOMY_ADMIN_LIFT_DONE, true).await;
            log(
                ctx,
                guild,
                ModLogEvent::ImmunityLifted {
                    by: ctx.author().id,
                    target: member.id,
                },
            )
            .await;
            replied
        },
        Ok(false) => say(ctx, ECONOMY_ADMIN_LIFT_NONE, true).await,
        Err(GrantError::NoDatabase) => say(ctx, ECONOMY_NO_DATABASE, true).await,
        Err(_) => say(ctx, ECONOMY_ADMIN_LIFT_FAILED, true).await,
    }
}

/// Add or remove credits (negative removes). Needs Manage Server.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn adjust(
    ctx: Context<'_>,
    #[description = "Who"] member: User,
    #[description = "Credits to add, or a negative number to remove"]
    #[min = -1000000000]
    #[max = 1000000000]
    amount: i64,
    #[description = "Why (shown in the mod log)"] reason: String,
) -> Result<(), Error> {
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    // For a slash command the member's `permissions` is the resolved set.
    let permitted = invoker_permissions(ctx).await.is_some_and(adjust_allowed);
    if !permitted {
        return say(ctx, ECONOMY_ADMIN_ADJUST_NEEDS_MANAGE, true).await;
    }
    if member.bot() {
        return say(ctx, ECONOMY_ADMIN_BOT_TARGET, true).await;
    }
    if !adjust_amount_ok(amount) {
        return say(ctx, ECONOMY_ADMIN_ADJUST_TOO_BIG, true).await;
    }
    let by = ctx.author().id;
    let cfg = ctx.data().economy.settings(guild).config;
    match db::adjust(&pool, guild, member.id, amount, by, &reason).await {
        Ok(balance) => {
            let replied = say(
                ctx,
                format!(
                    "{ECONOMY_ADMIN_ADJUST_DONE} {balance} {} {}.",
                    cfg.currency_emoji, cfg.currency_name
                ),
                true,
            )
            .await;
            log(
                ctx,
                guild,
                ModLogEvent::CreditsAdjusted {
                    by,
                    target: member.id,
                    amount,
                    reason,
                },
            )
            .await;
            replied
        },
        Err(MoveError::Insufficient { balance }) => {
            say(
                ctx,
                format!(
                    "{ECONOMY_ADMIN_ADJUST_BELOW_ZERO_PREFIX} {balance} {} {}.",
                    cfg.currency_emoji, cfg.currency_name
                ),
                true,
            )
            .await
        },
        Err(MoveError::Invalid) => say(ctx, ECONOMY_ADMIN_ADJUST_ZERO, true).await,
        Err(MoveError::Db(err)) => {
            tracing::warn!("economy: adjusting {} in {guild} failed: {err}", member.id);
            say(ctx, ECONOMY_ADMIN_ADJUST_FAILED, true).await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::GuildId;

    #[test]
    fn economy_status_text_exact() {
        let mut s = EconomySettings::off(GuildId::new(1));
        assert_eq!(
            economy_status_text(&s, 3, 1250),
            "**Economy status**: off\n\
             Currency: 🪙 credits\n\
             Wallets: 3\n\
             Held in total: 1250 🪙 credits\n\
             Earning: 1 per voice minute, 1 per message, at most once every 60s\n\
             Immunity: opt-out on, shop on, mod grants on\n\
             Gambling: off, max bet 500"
        );
        s.enabled = true;
        s.config.currency_name = "gems".into();
        s.config.currency_emoji = "💎".into();
        s.config.immunity_shop = false;
        s.config.gambling_enabled = true;
        let t = economy_status_text(&s, 0, 0);
        assert!(t.starts_with("**Economy status**: on\nCurrency: 💎 gems\n"));
        assert!(t.contains("shop off"));
        assert!(t.ends_with("Gambling: on, max bet 500"));
    }

    #[test]
    fn mod_allowed_needs_moderate_members() {
        assert!(!mod_allowed(Permissions::empty()));
        assert!(mod_allowed(Permissions::MODERATE_MEMBERS));
        assert!(mod_allowed(Permissions::ADMINISTRATOR));
        assert!(!mod_allowed(Permissions::MANAGE_GUILD));
    }

    #[test]
    fn hours_ok_bounds() {
        assert!(!hours_ok(0));
        assert!(hours_ok(1));
        assert!(hours_ok(8760));
        assert!(!hours_ok(8761));
    }

    #[test]
    fn adjust_allowed_needs_manage_guild() {
        assert!(!adjust_allowed(Permissions::MODERATE_MEMBERS));
        assert!(!adjust_allowed(Permissions::empty()));
        assert!(adjust_allowed(
            Permissions::MODERATE_MEMBERS | Permissions::MANAGE_GUILD
        ));
        assert!(adjust_allowed(Permissions::ADMINISTRATOR));
    }

    #[test]
    fn adjust_amount_ok_bounds() {
        assert!(adjust_amount_ok(1));
        assert!(adjust_amount_ok(1_000_000_000));
        assert!(adjust_amount_ok(-1_000_000_000));
        assert!(!adjust_amount_ok(1_000_000_001));
        assert!(!adjust_amount_ok(-1_000_000_001));
        assert!(!adjust_amount_ok(i64::MIN));
        assert!(!adjust_amount_ok(i64::MAX));
    }
}
