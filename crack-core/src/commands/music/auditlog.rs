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
use crate::music::audit::Source;
use crate::music::audit_view::{
    compose_auditlog, parse_since, premium_history_line, AuditlogReply, AUDITLOG_LIMIT,
};
use crate::utils::{create_paged_embed, PagedStyle};
use crate::{Context, Error};
use poise::serenity_prelude as serenity;

/// Show who changed this server's queue, and how.
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

/// The `action` option's choices: exactly the names the recorder writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum ActionChoice {
    Add,
    Remove,
    Move,
    Skip,
    Clear,
    Shuffle,
    Stop,
    Pause,
    Resume,
    Repeat,
    Leave,
}

impl ActionChoice {
    /// The stored `action` value this choice filters on.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ActionChoice::Add => "add",
            ActionChoice::Remove => "remove",
            ActionChoice::Move => "move",
            ActionChoice::Skip => "skip",
            ActionChoice::Clear => "clear",
            ActionChoice::Shuffle => "shuffle",
            ActionChoice::Stop => "stop",
            ActionChoice::Pause => "pause",
            ActionChoice::Resume => "resume",
            ActionChoice::Repeat => "repeat",
            ActionChoice::Leave => "leave",
        }
    }

    /// The choice whose stored name is `s` (lowercase, as recorded).
    #[must_use]
    pub fn from_name(s: &str) -> Option<ActionChoice> {
        Some(match s {
            "add" => ActionChoice::Add,
            "remove" => ActionChoice::Remove,
            "move" => ActionChoice::Move,
            "skip" => ActionChoice::Skip,
            "clear" => ActionChoice::Clear,
            "shuffle" => ActionChoice::Shuffle,
            "stop" => ActionChoice::Stop,
            "pause" => ActionChoice::Pause,
            "resume" => ActionChoice::Resume,
            "repeat" => ActionChoice::Repeat,
            "leave" => ActionChoice::Leave,
            _ => return None,
        })
    }
}

/// The `source` option's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum SourceChoice {
    Slash,
    Prefix,
    Web,
    Button,
    Bot,
}

impl SourceChoice {
    #[must_use]
    pub fn source(self) -> Source {
        match self {
            SourceChoice::Slash => Source::Slash,
            SourceChoice::Prefix => Source::Prefix,
            SourceChoice::Web => Source::Web,
            SourceChoice::Button => Source::Button,
            SourceChoice::Bot => Source::Bot,
        }
    }

    /// The choice whose stored spelling is `s`: `slash`, `prefix`, `web`, `button`, `bot`.
    #[must_use]
    pub fn from_name(s: &str) -> Option<SourceChoice> {
        Some(match s {
            "slash" => SourceChoice::Slash,
            "prefix" => SourceChoice::Prefix,
            "web" => SourceChoice::Web,
            "button" => SourceChoice::Button,
            "bot" => SourceChoice::Bot,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::audit::{Action, TrackRef};

    fn t(title: &str) -> TrackRef {
        TrackRef {
            title: Some(title.into()),
            url: None,
        }
    }

    #[test]
    fn action_choices_match_action_names() {
        let tr = t("a");
        let pairs = [
            (
                ActionChoice::Add,
                Action::Add {
                    tracks: vec![],
                    at: crate::music::audit::AddAt::Back,
                },
            ),
            (
                ActionChoice::Remove,
                Action::Remove {
                    track: tr.clone(),
                    index: 0,
                },
            ),
            (
                ActionChoice::Move,
                Action::Move {
                    track: tr.clone(),
                    from: 0,
                    to: 1,
                },
            ),
            (ActionChoice::Skip, Action::Skip { track: None }),
            (ActionChoice::Clear, Action::Clear { removed: 0 }),
            (ActionChoice::Shuffle, Action::Shuffle { count: 0 }),
            (ActionChoice::Stop, Action::Stop { removed: 0 }),
            (ActionChoice::Pause, Action::Pause),
            (ActionChoice::Resume, Action::Resume),
            (ActionChoice::Repeat, Action::Repeat { on: true }),
            (ActionChoice::Leave, Action::Leave { discarded: 0 }),
        ];
        for (c, a) in pairs {
            assert_eq!(c.name(), a.name());
        }
    }

    #[test]
    fn source_choices_match_source_names() {
        for (c, s) in [
            (SourceChoice::Slash, Source::Slash),
            (SourceChoice::Prefix, Source::Prefix),
            (SourceChoice::Web, Source::Web),
            (SourceChoice::Button, Source::Button),
            (SourceChoice::Bot, Source::Bot),
        ] {
            assert_eq!(c.source(), s);
        }
    }

    #[test]
    fn choices_round_trip_through_their_names() {
        for c in [
            ActionChoice::Add,
            ActionChoice::Remove,
            ActionChoice::Move,
            ActionChoice::Skip,
            ActionChoice::Clear,
            ActionChoice::Shuffle,
            ActionChoice::Stop,
            ActionChoice::Pause,
            ActionChoice::Resume,
            ActionChoice::Repeat,
            ActionChoice::Leave,
        ] {
            assert_eq!(ActionChoice::from_name(c.name()), Some(c));
        }
        assert_eq!(ActionChoice::from_name("dance"), None);
        assert_eq!(
            ActionChoice::from_name("Move"),
            None,
            "stored names are lowercase"
        );
        for c in [
            SourceChoice::Slash,
            SourceChoice::Prefix,
            SourceChoice::Web,
            SourceChoice::Button,
            SourceChoice::Bot,
        ] {
            assert_eq!(SourceChoice::from_name(c.source().as_str()), Some(c));
        }
        assert_eq!(SourceChoice::from_name("email"), None);
    }
}
