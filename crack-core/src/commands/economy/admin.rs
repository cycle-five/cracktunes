//! `/economy status|grant|lift|adjust`, for moderators.
use super::credits::{gate, gate_for, live, say, GateNeed};
use crate::db::economy::{self as db, RecentAction};
use crate::economy::preflight::{report_for, ActionPreflight};
use crate::economy::settings::EconomySettings;
use crate::economy::wallet::{ActionState, MoveError};
use crate::economy::{mod_cancel, mod_grant, mod_lift, GrantError};
use crate::messaging::format::CONTENT_MAX;
use crate::messaging::messages::{
    DASH_ECONOMY_ANNOUNCE_STATUS, DASH_ECONOMY_PREFLIGHT, DASH_ECONOMY_PREFLIGHT_READY,
    DASH_ECONOMY_PREFLIGHT_UNKNOWN, ECONOMY_ADMIN_ADJUST_BELOW_ZERO_PREFIX,
    ECONOMY_ADMIN_ADJUST_DONE, ECONOMY_ADMIN_ADJUST_FAILED, ECONOMY_ADMIN_ADJUST_NEEDS_MANAGE,
    ECONOMY_ADMIN_ADJUST_TOO_BIG, ECONOMY_ADMIN_ADJUST_ZERO, ECONOMY_ADMIN_BOT_TARGET,
    ECONOMY_ADMIN_GRANT_DISABLED, ECONOMY_ADMIN_GRANT_DONE, ECONOMY_ADMIN_GRANT_FAILED,
    ECONOMY_ADMIN_HOURS_RANGE, ECONOMY_ADMIN_LIFT_DONE, ECONOMY_ADMIN_LIFT_FAILED,
    ECONOMY_ADMIN_LIFT_NONE, ECONOMY_ADMIN_NEEDS_MODERATE, ECONOMY_CANCEL_DONE_PREFIX,
    ECONOMY_CANCEL_DONE_SUFFIX, ECONOMY_CANCEL_NONE, ECONOMY_NO_DATABASE, ECONOMY_READ_FAILED,
    ECONOMY_STATUS_CURRENCY, ECONOMY_STATUS_EARNING, ECONOMY_STATUS_GAMBLING, ECONOMY_STATUS_HELD,
    ECONOMY_STATUS_IMMUNITY, ECONOMY_STATUS_MAX_BET, ECONOMY_STATUS_MODS, ECONOMY_STATUS_OFF,
    ECONOMY_STATUS_ON, ECONOMY_STATUS_PER_MESSAGE, ECONOMY_STATUS_PER_VOICE_MINUTE,
    ECONOMY_STATUS_RECENT, ECONOMY_STATUS_RECENT_MORE_PREFIX, ECONOMY_STATUS_RECENT_MORE_SUFFIX,
    ECONOMY_STATUS_RECENT_NONE, ECONOMY_STATUS_SECONDS, ECONOMY_STATUS_SHOP, ECONOMY_STATUS_TITLE,
    ECONOMY_STATUS_UNTIL, ECONOMY_STATUS_WALLETS,
};
use crate::messaging::transport::DiscordTransport;
use crate::modlog::batch::ModLogSink;
use crate::modlog::event::ModLogEvent;
use crate::modlog::live::ChannelSink;
use crate::{Context, Data, Error};
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

/// How many past actions `/economy status` lists.
pub const STATUS_RECENT: i64 = 10;

/// What one prank (or the announcements) needs, in the dashboard's words:
/// "Ready.", the gaps, or that it couldn't be checked.
fn needs_words(needs: Option<&[String]>) -> String {
    match needs {
        None => DASH_ECONOMY_PREFLIGHT_UNKNOWN.to_string(),
        Some([]) => DASH_ECONOMY_PREFLIGHT_READY.to_string(),
        Some(gaps) => gaps.join("; "),
    }
}

/// The permission report, one line per prank, then the announcement
/// channel's when one is set and could be checked.
pub fn preflight_lines(report: &[ActionPreflight], announce: Option<&[String]>) -> Vec<String> {
    let mut lines: Vec<String> = report
        .iter()
        .map(|a| format!("{}: {}", a.label, needs_words(a.needs.as_deref())))
        .collect();
    if let Some(gaps) = announce {
        lines.push(format!(
            "{DASH_ECONOMY_ANNOUNCE_STATUS}: {}",
            needs_words(Some(gaps))
        ));
    }
    lines
}

/// One past action: kind, buyer, target, state, then when it ends (while it
/// runs) or why it ended. The buyer is always named, signed or not: this is
/// for moderators, privately (spec 5.4: the record when the mod log is off).
pub fn recent_line(a: &RecentAction) -> String {
    let r = &a.row;
    let mut line = format!("{} · <@{}>", r.kind.label(), r.buyer);
    if let Some(target) = r.target {
        line.push_str(&format!(" → <@{target}>"));
    }
    line.push_str(&format!(" · {}", r.state.as_str()));
    match r.state {
        ActionState::Pending | ActionState::Funding | ActionState::Active | ActionState::Ending => {
            if let Some(until) = r.expires_at {
                line.push_str(&format!(
                    " · {ECONOMY_STATUS_UNTIL} <t:{}:R>",
                    until.timestamp()
                ));
            }
        },
        ActionState::Done | ActionState::Ended | ActionState::Failed => {
            if let Some(why) = &a.end_reason {
                line.push_str(&format!(" · {why}"));
            }
        },
    }
    line
}

/// A message's length as Discord limits it. Counted in UTF-16 units, which
/// is never fewer than the characters Discord counts, so a text within
/// [`CONTENT_MAX`] of these is within Discord's limit either way.
fn discord_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s` cut to at most `max` [`discord_len`] units, ending in "…" when cut.
fn fit(s: &str, max: usize) -> String {
    if discord_len(s) <= max {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        if used + c.len_utf16() > max.saturating_sub(1) {
            break;
        }
        used += c.len_utf16();
        out.push(c);
    }
    out.push('…');
    out
}

/// `/economy status` in full: the settings, the permission report, and the
/// last actions newest first (`None` when they couldn't be read). Discord
/// refuses a message over [`CONTENT_MAX`], so when the whole would be longer
/// the oldest actions are left out, with a line saying how many.
pub fn status_report_text(
    base: &str,
    report: &[ActionPreflight],
    announce: Option<&[String]>,
    recent: Option<&[RecentAction]>,
) -> String {
    let mut head = vec![base.to_string(), format!("**{DASH_ECONOMY_PREFLIGHT}**")];
    head.extend(preflight_lines(report, announce));
    head.push(format!("**{ECONOMY_STATUS_RECENT}**"));
    let rows: Vec<String> = match recent {
        None => vec![ECONOMY_READ_FAILED.to_string()],
        Some([]) => vec![ECONOMY_STATUS_RECENT_NONE.to_string()],
        Some(rows) => rows.iter().map(recent_line).collect(),
    };
    let actions = recent.map_or(0, <[RecentAction]>::len);
    // Newest first, so keeping a prefix drops from the oldest end.
    for keep in (0..=rows.len()).rev() {
        let mut lines = head.clone();
        lines.extend(rows[..keep].iter().cloned());
        let left_out = rows.len() - keep;
        if left_out > 0 && actions > 0 {
            lines.push(format!(
                "{ECONOMY_STATUS_RECENT_MORE_PREFIX} {left_out} {ECONOMY_STATUS_RECENT_MORE_SUFFIX}"
            ));
        }
        let text = lines.join("\n");
        if discord_len(&text) <= CONTENT_MAX {
            return text;
        }
    }
    // Even the head alone is too long (it never is in practice): cut it.
    fit(&head.join("\n"), CONTENT_MAX)
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
pub(super) async fn log(ctx: Context<'_>, guild: GuildId, event: ModLogEvent) {
    log_to(&ctx.data(), ctx.serenity_context(), guild, event).await;
}

/// [`log`] without a command context (a button or modal press).
#[cfg(not(tarpaulin_include))]
pub(super) async fn log_to(
    data: &Data,
    ctx: &serenity::all::Context,
    guild: GuildId,
    event: ModLogEvent,
) {
    let sink = ChannelSink {
        data: data.clone(),
        transport: Arc::new(DiscordTransport::of(ctx)),
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
    subcommands("status", "grant", "lift", "cancel", "adjust"),
    subcommand_required
)]
pub async fn economy(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Show how the economy is set up here.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    // Needs only the database: moderators keep it with the economy off.
    let Some((guild, pool)) = gate_for(ctx, GateNeed::DatabaseOnly).await? else {
        return Ok(());
    };
    if !mod_gate(ctx).await? {
        return Ok(());
    }
    let (stats, recent) = tokio::join!(
        db::stats(&pool, guild),
        db::recent_actions(&pool, guild, STATUS_RECENT)
    );
    let recent = recent
        .inspect_err(|err| tracing::warn!("economy: recent actions of {guild} failed: {err}"))
        .ok();
    match stats {
        Ok((wallets, credits)) => {
            let s = ctx.data().economy.settings(guild);
            let (report, announce) = report_for(&ctx.serenity_context().cache, guild, &s.config);
            let text = status_report_text(
                &economy_status_text(&s, wallets, credits),
                &report,
                announce.as_deref(),
                recent.as_deref(),
            );
            say(ctx, text, true).await
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
    // A grant ends the pranks on them, one Discord call each.
    ctx.defer_ephemeral().await?;
    if member.bot() {
        return say(ctx, ECONOMY_ADMIN_BOT_TARGET, true).await;
    }
    if hours.is_some_and(|h| !hours_ok(h)) {
        return say(ctx, ECONOMY_ADMIN_HOURS_RANGE, true).await;
    }
    let until = hours.map(|h| Utc::now() + Duration::hours(i64::from(h)));
    let by = ctx.author().id;
    match mod_grant(
        &ctx.data(),
        &live(ctx),
        guild,
        member.id,
        until,
        by,
        reason.as_deref(),
    )
    .await
    {
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

/// End every prank running on a member, undone as if it had expired.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only)]
pub async fn cancel(ctx: Context<'_>, #[description = "Who"] member: User) -> Result<(), Error> {
    // Needs only the database: the lever for pranks still running after the
    // economy was turned off.
    let Some((guild, _pool)) = gate_for(ctx, GateNeed::DatabaseOnly).await? else {
        return Ok(());
    };
    if !mod_gate(ctx).await? {
        return Ok(());
    }
    // One Discord call per prank: past Discord's three seconds otherwise.
    ctx.defer_ephemeral().await?;
    match mod_cancel(&ctx.data(), &live(ctx), guild, member.id, Utc::now()).await {
        Ok(0) => say(ctx, ECONOMY_CANCEL_NONE, true).await,
        Ok(n) => {
            let replied = say(
                ctx,
                format!("{ECONOMY_CANCEL_DONE_PREFIX} {n} {ECONOMY_CANCEL_DONE_SUFFIX}"),
                true,
            )
            .await;
            log(
                ctx,
                guild,
                ModLogEvent::ShopCancelled {
                    by: ctx.author().id,
                    target: member.id,
                },
            )
            .await;
            replied
        },
        Err(GrantError::NoDatabase) => say(ctx, ECONOMY_NO_DATABASE, true).await,
        Err(_) => say(ctx, ECONOMY_READ_FAILED, true).await,
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

    fn recent(
        kind: crate::economy::settings::ActionKind,
        state: ActionState,
        signed: bool,
        expires_at: Option<chrono::DateTime<Utc>>,
        end_reason: Option<&str>,
    ) -> RecentAction {
        use crate::economy::wallet::ActionPayload;
        let at = chrono::TimeZone::timestamp_opt(&Utc, 1_800_000_000, 0).unwrap();
        RecentAction {
            row: db::ActionRow {
                id: 1,
                kind,
                buyer: serenity::all::UserId::new(7),
                target: Some(serenity::all::UserId::new(8)),
                payload: ActionPayload::Mute,
                state,
                expires_at,
                note: None,
                signed,
                revealed_at: None,
            },
            end_reason: end_reason.map(str::to_owned),
            bought_at: at,
        }
    }

    #[test]
    fn status_names_the_buyer_of_a_running_unsigned_prank() {
        use crate::economy::settings::ActionKind;
        let at = chrono::TimeZone::timestamp_opt(&Utc, 1_800_000_000, 0).unwrap();
        let rows = [
            recent(ActionKind::Mute, ActionState::Active, false, Some(at), None),
            recent(
                ActionKind::Rename,
                ActionState::Ended,
                false,
                Some(at),
                Some("cleansed"),
            ),
            recent(ActionKind::Drag, ActionState::Done, true, None, None),
        ];
        let text = status_report_text("BASE", &[], None, Some(&rows));
        assert_eq!(
            text,
            "BASE\n\
             **What CrackTunes needs for each prank**\n\
             **Recent shop actions**\n\
             Mute · <@7> → <@8> · active · until <t:1800000000:R>\n\
             Rename · <@7> → <@8> · ended · cleansed\n\
             Drag · <@7> → <@8> · done"
        );
        assert_eq!(
            status_report_text("BASE", &[], None, Some(&[])),
            "BASE\n**What CrackTunes needs for each prank**\n**Recent shop actions**\nNothing bought yet."
        );
        assert!(status_report_text("BASE", &[], None, None)
            .ends_with("**Recent shop actions**\nCouldn't read that right now. Try again."));
    }

    #[test]
    fn status_fits_in_one_message_dropping_the_oldest_actions() {
        use crate::economy::settings::ActionKind;
        let at = chrono::TimeZone::timestamp_opt(&Utc, 1_800_000_000, 0).unwrap();
        // A long base, every preflight line long, and ten long, wide actions
        // (these emoji take two UTF-16 units each).
        let base = "B".repeat(400);
        let gap = format!(
            "Give CrackTunes {} in Server Settings → Roles",
            "🔒".repeat(40)
        );
        let report: Vec<ActionPreflight> = ActionKind::PRANKS
            .iter()
            .map(|k| ActionPreflight {
                kind: k.key().to_owned(),
                label: k.label().to_owned(),
                needs: Some(vec![gap.clone()]),
            })
            .collect();
        let announce = vec![gap.clone()];
        let rows: Vec<RecentAction> = (0..10)
            .map(|n| {
                let mut r = recent(
                    ActionKind::Rename,
                    ActionState::Ended,
                    false,
                    Some(at),
                    Some(&format!("reason-{n}-{}", "🧼".repeat(60))),
                );
                r.row.id = 100 - n;
                r
            })
            .collect();
        let text = status_report_text(&base, &report, Some(&announce), Some(&rows));
        // Measured independently, in UTF-16 units: the most Discord can count.
        let units = |s: &str| s.encode_utf16().count();
        assert!(units(&text) <= 2000, "{}", units(&text));
        // The newest is kept; the oldest went, and the reply says how many.
        assert!(text.contains("reason-0-"), "the newest action is shown");
        assert!(!text.contains("reason-9-"), "the oldest is left out");
        let shown = (0..10)
            .filter(|n| text.contains(&format!("reason-{n}-")))
            .count();
        assert!(
            text.ends_with(&format!("…and {} older not shown.", 10 - shown)),
            "{text}"
        );
        assert!(shown >= 1 && shown < 10, "{shown}");
        // A head longer than any message is cut, never sent too long.
        let huge = status_report_text(&"B".repeat(3000), &report, None, None);
        assert!(units(&huge) <= 2000);
        assert!(huge.ends_with('…'));
    }

    #[test]
    fn status_shows_the_permission_report_in_the_dashboards_words() {
        let pf = |label: &str, needs: Option<Vec<&str>>| ActionPreflight {
            kind: label.to_lowercase(),
            label: label.to_owned(),
            needs: needs.map(|v| v.into_iter().map(str::to_owned).collect()),
        };
        let report = [
            pf("Rename", Some(vec![])),
            pf(
                "Lock nickname",
                Some(vec![
                    "Give CrackTunes View Audit Log in Server Settings → Roles",
                ]),
            ),
            pf("Jail", Some(vec!["Give CrackTunes Connect in <#9>", "x"])),
            pf("Mute", None),
        ];
        let gaps = vec!["Give CrackTunes Embed Links in <#5>".to_owned()];
        assert_eq!(
            preflight_lines(&report, Some(&gaps)),
            vec![
                "Rename: Ready.",
                "Lock nickname: Give CrackTunes View Audit Log in Server Settings → Roles",
                "Jail: Give CrackTunes Connect in <#9>; x",
                "Mute: Couldn't check permissions right now.",
                "Announcements: Give CrackTunes Embed Links in <#5>",
            ]
        );
        assert_eq!(
            preflight_lines(&report[..1], Some(&[])),
            vec!["Rename: Ready.", "Announcements: Ready."]
        );
        assert_eq!(preflight_lines(&report[..1], None), vec!["Rename: Ready."]);
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
