//! `/premium grant|revoke <server_id>`: a bot owner turns premium on or off
//! for any server the bot is in, from wherever they run it.
//!
//! 🔑 Premium lives in two places: the in-memory settings, which everything
//! reads, and `guild_settings.premium`, which a restart loads. Both are written
//! here. Editing the column by hand while the bot runs does not stick: memory
//! keeps the old value, and the shutdown save writes it back over the edit.

use crate::db::GuildEntity;
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::messages::{
    PREMIUM_BAD_SERVER_ID, PREMIUM_FAILED, PREMIUM_GRANTED, PREMIUM_NOT_IN_SERVER, PREMIUM_REVOKED,
};
use crate::poise_ext::ContextExt;
use crate::{Context, Error};
use poise::serenity_prelude as serenity;
use poise::CreateReply;
use std::num::NonZeroU64;

/// Turn premium on or off for a server (bot owners only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    slash_command,
    owners_only,
    // Hides it from everyone but administrators in Discord's command list.
    // `owners_only` is what actually guards it.
    default_member_permissions = "ADMINISTRATOR",
    subcommands("grant", "revoke"),
    subcommand_required,
    ephemeral
)]
pub async fn premium(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Give a server premium.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, owners_only, ephemeral)]
pub async fn grant(
    ctx: Context<'_>,
    #[description = "The server's id"] server_id: String,
) -> Result<(), Error> {
    set_premium_for(ctx, &server_id, true).await
}

/// Take premium away from a server.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, owners_only, ephemeral)]
pub async fn revoke(
    ctx: Context<'_>,
    #[description = "The server's id"] server_id: String,
) -> Result<(), Error> {
    set_premium_for(ctx, &server_id, false).await
}

/// A Discord server id as typed: a positive whole number, spaces around it allowed.
#[must_use]
pub fn parse_server_id(s: &str) -> Option<serenity::GuildId> {
    let n: u64 = s.trim().parse().ok()?;
    NonZeroU64::new(n).map(|n| serenity::GuildId::new(n.get()))
}

#[cfg(not(tarpaulin_include))]
async fn set_premium_for(ctx: Context<'_>, server_id: &str, premium: bool) -> Result<(), Error> {
    let say = |text: String| ctx.send(CreateReply::default().content(text).ephemeral(true));

    let Some(guild_id) = parse_server_id(server_id) else {
        say(PREMIUM_BAD_SERVER_ID.to_owned()).await?;
        return Ok(());
    };
    // Only servers the bot is in: a mistyped id must not create settings for
    // a server that doesn't exist.
    let Some(name) = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .map(|g| g.name.to_string())
    else {
        say(PREMIUM_NOT_IN_SERVER.to_owned()).await?;
        return Ok(());
    };

    let data = ctx.data();
    // Load the stored row first, so memory holds real settings (not defaults)
    // before it changes, and the row exists for the update below.
    if let Err(e) = data.ensure_settings_loaded(guild_id).await {
        tracing::warn!("/premium: could not load settings for {guild_id}: {e}");
        say(PREMIUM_FAILED.to_owned()).await?;
        return Ok(());
    }
    let pool = ctx.get_db_pool()?;
    // The database first: if it fails, memory still matches what is stored.
    if let Err(e) = GuildEntity::update_premium(&pool, guild_id.get() as i64, premium).await {
        tracing::warn!("/premium: could not store premium={premium} for {guild_id}: {e}");
        say(PREMIUM_FAILED.to_owned()).await?;
        return Ok(());
    }
    data.set_premium(guild_id, premium).await;
    tracing::info!(
        "/premium: premium={premium} for {guild_id} by {}",
        ctx.author().id
    );

    let done = if premium {
        PREMIUM_GRANTED
    } else {
        PREMIUM_REVOKED
    };
    say(format!("{done} {name} (`{guild_id}`).")).await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_server_id_is_a_positive_whole_number() {
        assert_eq!(
            parse_server_id("1267282599064244255"),
            Some(serenity::GuildId::new(1267282599064244255))
        );
        assert_eq!(
            parse_server_id("  1267282599064244255 "),
            Some(serenity::GuildId::new(1267282599064244255))
        );
        assert_eq!(parse_server_id("0"), None);
        assert_eq!(parse_server_id("-5"), None);
        assert_eq!(parse_server_id("my server"), None);
        assert_eq!(parse_server_id(""), None);
        assert_eq!(parse_server_id("99999999999999999999"), None);
    }

    /// The whole guard: anyone may see the command in a server they
    /// administer, but only a bot owner may run it. Poise checks a parent's
    /// `owners_only` before a subcommand's, and each is marked anyway.
    #[test]
    fn premium_and_its_subcommands_are_owners_only() {
        let cmd = premium();
        assert!(cmd.owners_only);
        let names: Vec<&str> = cmd.subcommands.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, vec!["grant", "revoke"]);
        assert!(cmd.subcommands.iter().all(|c| c.owners_only));
    }

    #[test]
    fn premium_is_registered() {
        assert!(crate::commands::all_command_names()
            .iter()
            .any(|n| n == "premium"));
    }
}
