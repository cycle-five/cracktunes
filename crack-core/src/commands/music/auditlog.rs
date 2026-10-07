//! `/auditlog`: the queue history, for members with Manage Server.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

use crate::db::queue_audit::{recent_audit, AuditFilter};
use crate::guild::operations::GuildSettingsOperations;
use crate::guild::plan::Plan;
use crate::messaging::courier;
use crate::messaging::messages::{
    AUDITLOG_BAD_SINCE, AUDITLOG_EMPTY, AUDITLOG_FAILED, AUDITLOG_GP_HIDDEN, AUDITLOG_NO_DATABASE,
    AUDITLOG_ONLY_OLDER, AUDITLOG_TITLE,
};
use crate::messaging::render::Rendered;
use crate::music::audit_view::{
    compose_auditlog, parse_since, premium_history_line, ActionChoice, AuditlogReply, SourceChoice,
    AUDITLOG_LIMIT,
};
use crate::utils::{create_paged_embed, PagedStyle};
use crate::{Context, Error};
use poise::serenity_prelude as serenity;

/// Show who changed this server's queue, and how.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD"
)]
pub async fn auditlog(
    ctx: Context<'_>,
    #[description = "Only this member's changes"] user: Option<serenity::User>,
    #[description = "Only this kind of change"] action: Option<ActionChoice>,
    #[description = "Only changes made this way"] source: Option<SourceChoice>,
    #[description = "Only changes this recent: 90m, 6h, 2d, 1w"] since: Option<String>,
    #[description = "Post it in the channel for everyone (default: only you see it)"]
    public: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(crate::CrackedError::NoGuildId)?;
    let ephemeral = !public.unwrap_or(false);
    // Mistakes in the request are answered privately whatever `public` says:
    // nobody else needs to see a typo. Both are checked before the defer, while
    // the interaction has no response yet.
    let tell_caller = |text: &'static str| courier::reply_rendered(ctx, Rendered::text(text), true);

    let Some(pool) = ctx.data().database_pool.clone() else {
        tell_caller(AUDITLOG_NO_DATABASE).await?;
        return Ok(());
    };
    let since = match since.as_deref().map(parse_since) {
        None => None,
        Some(Some(d)) => Some(chrono::Utc::now() - d),
        Some(None) => {
            tell_caller(AUDITLOG_BAD_SINCE).await?;
            return Ok(());
        },
    };
    // The query can outlast Discord's 3-second window. The defer fixes where the
    // answer appears: privately unless `public` was asked for, and every reply
    // below follows it.
    if ephemeral {
        ctx.defer_ephemeral().await?;
    } else {
        ctx.defer().await?;
    }
    let say = |text: &'static str| courier::reply_rendered(ctx, Rendered::text(text), ephemeral);
    let source_str = source.map(|s| s.source().as_str());
    let filter = AuditFilter {
        user: user.map(|u| u.id),
        action: action.map(ActionChoice::name),
        source: source_str,
        since,
    };
    let rows = match recent_audit(&pool, guild_id, &filter, AUDITLOG_LIMIT as i64 + 1).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("/auditlog: reading the queue history failed: {e}");
            say(AUDITLOG_FAILED).await?;
            return Ok(());
        },
    };

    // Copy the start time out so the DashMap ref is dropped before any await.
    let running_since = ctx
        .data()
        .gp_games
        .get(&guild_id)
        .map(|g| g.started_at)
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0));
    let floor = Plan::of(ctx.data().get_premium(guild_id).await).history_floor(chrono::Utc::now());
    let lines = match compose_auditlog(rows, running_since, floor) {
        AuditlogReply::Empty => {
            say(AUDITLOG_EMPTY).await?;
            return Ok(());
        },
        AuditlogReply::AllHidden => {
            say(AUDITLOG_GP_HIDDEN).await?;
            return Ok(());
        },
        AuditlogReply::OnlyOlder => {
            let text = format!("{AUDITLOG_ONLY_OLDER}\n{}", premium_history_line());
            courier::reply_rendered(ctx, Rendered::text(text), ephemeral).await?;
            return Ok(());
        },
        AuditlogReply::Lines(lines) => lines,
    };

    create_paged_embed(
        ctx,
        ctx.author().name.clone(),
        AUDITLOG_TITLE.to_owned(),
        lines.join("\n"),
        900,
        PagedStyle {
            ephemeral,
            fenced: false,
        },
    )
    .await?;
    Ok(())
}
