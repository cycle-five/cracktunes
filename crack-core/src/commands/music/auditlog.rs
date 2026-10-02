//! `/auditlog`: the queue history, for members with Manage Server.
//! Spec: docs/superpowers/specs/2026-10-02-auditlog-command-design.md

use crate::db::queue_audit::{recent_audit, AuditFilter};
use crate::messaging::messages::{
    AUDITLOG_BAD_SINCE, AUDITLOG_EMPTY, AUDITLOG_FAILED, AUDITLOG_GP_HIDDEN, AUDITLOG_NO_DATABASE,
    AUDITLOG_TITLE,
};
use crate::music::audit_view::{
    compose_auditlog, parse_since, ActionChoice, AuditlogReply, SourceChoice, AUDITLOG_LIMIT,
};
use crate::utils::{create_paged_embed, PagedStyle};
use crate::{Context, Error};
use poise::serenity_prelude as serenity;
use poise::CreateReply;

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
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(crate::CrackedError::NoGuildId)?;
    let say = |text: &'static str| ctx.send(CreateReply::default().content(text).ephemeral(true));

    let Some(pool) = ctx.data().database_pool.clone() else {
        say(AUDITLOG_NO_DATABASE).await?;
        return Ok(());
    };
    // The query can outlast Discord's 3-second window. After an ephemeral defer,
    // poise's `ctx.send` follows up with `create_followup`, which keeps the
    // ephemeral flag, so every reply below stays private.
    ctx.defer_ephemeral().await?;
    let since = match since.as_deref().map(parse_since) {
        None => None,
        Some(Some(d)) => Some(chrono::Utc::now() - d),
        Some(None) => {
            say(AUDITLOG_BAD_SINCE).await?;
            return Ok(());
        },
    };
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
    let lines = match compose_auditlog(rows, running_since) {
        AuditlogReply::Empty => {
            say(AUDITLOG_EMPTY).await?;
            return Ok(());
        },
        AuditlogReply::AllHidden => {
            say(AUDITLOG_GP_HIDDEN).await?;
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
            ephemeral: true,
            fenced: false,
        },
    )
    .await?;
    Ok(())
}
