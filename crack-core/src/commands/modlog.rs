//! `/modlog set|off|status`: a server manager picks the channel the mod log
//! posts to, turns it off, and checks that CrackTunes can run it.
//!
//! Settings change only through `modlog::save_settings`, the same path the
//! dashboard uses.
use crate::commands::rooms::admin::dashboard_link;
use crate::messaging::courier;
use crate::messaging::messages::{
    MODLOG_NO_CHANNEL, MODLOG_NO_DATABASE, MODLOG_SETTINGS_NOT_LOADED, MODLOG_SETTINGS_SAVE_FAILED,
    MODLOG_STATUS_DASHBOARD, MODLOG_STATUS_OFF, MODLOG_STATUS_ON, MODLOG_STATUS_READY,
    MODLOG_STATUS_UNKNOWN, MODLOG_STATUS_VISIBLE_PREFIX, MODLOG_STATUS_VISIBLE_SUFFIX,
};
use crate::messaging::render::Rendered;
use crate::modlog::preflight::{self, finding_text, ModLogFinding};
use crate::modlog::settings::{ModLogSettings, ModLogSettingsError};
use crate::modlog::{save_settings, ModLogSaveError};
use crate::{Context, Error};
use serenity::all::{GenericChannelId, GuildId};

/// The one command.
pub fn commands() -> Vec<crate::Command> {
    vec![modlog()]
}

/// Set up, turn off, or check the mod log.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Mod log",
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    required_permissions = "MANAGE_GUILD",
    subcommands("set", "off", "status"),
    subcommand_required
)]
pub async fn modlog(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// What to tell a manager whose settings were not saved.
pub(crate) fn save_error_text(err: ModLogSaveError) -> &'static str {
    match err {
        ModLogSaveError::Invalid(ModLogSettingsError::NoDatabase) => MODLOG_NO_DATABASE,
        ModLogSaveError::Invalid(ModLogSettingsError::NoChannel) => MODLOG_NO_CHANNEL,
        ModLogSaveError::NotLoaded => MODLOG_SETTINGS_NOT_LOADED,
        ModLogSaveError::Db => MODLOG_SETTINGS_SAVE_FAILED,
    }
}

/// The status reply: settings, what is missing, who can read, dashboard.
pub(crate) fn status_text(
    settings: &ModLogSettings,
    findings: Option<&[ModLogFinding]>,
    dashboard: Option<&str>,
) -> String {
    let mut lines = vec![match (settings.enabled, settings.channel) {
        (true, Some(ch)) => format!("{MODLOG_STATUS_ON} <#{ch}>"),
        _ => MODLOG_STATUS_OFF.to_string(),
    }];
    match findings {
        None => lines.push(MODLOG_STATUS_UNKNOWN.to_string()),
        Some([]) => lines.push(MODLOG_STATUS_READY.to_string()),
        Some(list) => lines.extend(
            list.iter()
                .map(|f| finding_text(f, |c| format!("<#{c}>"), true)),
        ),
    }
    if let Some(ch) = settings.channel {
        lines.push(format!(
            "{MODLOG_STATUS_VISIBLE_PREFIX} <#{ch}> {MODLOG_STATUS_VISIBLE_SUFFIX}"
        ));
    }
    if let Some(url) = dashboard {
        lines.push(String::new());
        lines.push(format!("{MODLOG_STATUS_DASHBOARD} {url}"));
    }
    lines.join("\n")
}

#[cfg(not(tarpaulin_include))]
fn current_status(ctx: Context<'_>, guild: GuildId) -> String {
    let settings = ctx.data().modlog.settings(guild);
    let findings = preflight::for_guild(&ctx.serenity_context().cache, &ctx.data(), guild);
    status_text(
        &settings,
        findings.as_deref(),
        dashboard_link(std::env::var("WEB_PUBLIC_ORIGIN").ok().as_deref(), guild).as_deref(),
    )
}

#[cfg(not(tarpaulin_include))]
async fn say(ctx: Context<'_>, text: impl Into<String>) -> Result<(), Error> {
    courier::reply_rendered(ctx, Rendered::text(text), true).await?;
    Ok(())
}

/// Save `settings`; on success seed the audit snapshots (M20) and show status.
#[cfg(not(tarpaulin_include))]
async fn save_and_say(
    ctx: Context<'_>,
    guild: GuildId,
    settings: ModLogSettings,
) -> Result<(), Error> {
    match save_settings(&ctx.data(), settings).await {
        Ok(()) => {
            crate::modlog::live::after_save(
                &ctx.data(),
                ctx.serenity_context().http.clone(),
                guild,
            );
            say(ctx, current_status(ctx, guild)).await
        },
        Err(err) => say(ctx, save_error_text(err)).await,
    }
}

/// Turn the mod log on, posting to a text channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn set(
    ctx: Context<'_>,
    #[description = "The channel the log is posted to"]
    #[channel_types("Text")]
    channel: GenericChannelId,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    let settings = ModLogSettings {
        enabled: true,
        channel: Some(channel.expect_channel()),
        ..ctx.data().modlog.settings(guild)
    };
    save_and_say(ctx, guild, settings).await
}

/// Turn the mod log off.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn off(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    let settings = ModLogSettings {
        enabled: false,
        ..ctx.data().modlog.settings(guild)
    };
    save_and_say(ctx, guild, settings).await
}

/// Show the mod log settings and what CrackTunes still needs.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    say(ctx, current_status(ctx, guild)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude::all::{ChannelId, ChannelType, Permissions};

    #[test]
    fn modlog_is_manage_guild_only() {
        let command = modlog();
        assert_eq!(command.name, "modlog");
        assert!(command.slash_action.is_some(), "a slash command");
        assert!(command.guild_only, "guild only");
        assert!(command
            .required_permissions
            .contains(Permissions::MANAGE_GUILD));
        assert!(command
            .default_member_permissions
            .contains(Permissions::MANAGE_GUILD));
        assert!(
            crate::commands::commands_to_register()
                .into_iter()
                .any(|c| c.name == "modlog"),
            "registered"
        );
        let mut subs: Vec<_> = command
            .subcommands
            .iter()
            .map(|s| s.name.as_ref())
            .collect();
        subs.sort();
        assert_eq!(subs, ["off", "set", "status"]);
        assert!(command
            .subcommands
            .iter()
            .all(|s| s.required_permissions.contains(Permissions::MANAGE_GUILD)));
    }

    #[test]
    fn set_channel_is_text_only() {
        let command = modlog();
        let set = command
            .subcommands
            .iter()
            .find(|s| s.name == "set")
            .unwrap();
        let ch = set.parameters.iter().find(|p| p.name == "channel").unwrap();
        assert!(ch.required);
        assert_eq!(ch.channel_types.as_deref(), Some(&[ChannelType::Text][..]));
    }

    #[test]
    fn save_errors_have_their_own_words() {
        assert_eq!(
            save_error_text(ModLogSaveError::Invalid(ModLogSettingsError::NoDatabase)),
            "The mod log needs a database."
        );
        assert_eq!(
            save_error_text(ModLogSaveError::Invalid(ModLogSettingsError::NoChannel)),
            "Pick a channel for the log."
        );
        assert_eq!(
            save_error_text(ModLogSaveError::NotLoaded),
            "Mod log settings couldn't be read from the database yet — try again in a minute."
        );
        assert_eq!(
            save_error_text(ModLogSaveError::Db),
            "Couldn't save the mod log settings — try again."
        );
    }

    fn on() -> ModLogSettings {
        ModLogSettings {
            enabled: true,
            channel: Some(ChannelId::new(55)),
            ..ModLogSettings::off(GuildId::new(1))
        }
    }

    #[test]
    fn status_when_ready_names_the_channel_and_who_can_read() {
        assert_eq!(
            status_text(&on(), Some(&[]), Some("https://d.example/g/1/settings")),
            "Mod log: **on** in <#55>\n\
             CrackTunes has everything it needs.\n\
             Anyone who can see <#55> can read the log.\n\
             \n\
             Dashboard settings: https://d.example/g/1/settings"
        );
    }

    #[test]
    fn status_lists_findings_and_off_state() {
        let off = ModLogSettings {
            enabled: false,
            ..on()
        };
        let findings = [
            ModLogFinding::MissingServer(Permissions::VIEW_AUDIT_LOG),
            ModLogFinding::Suspended,
        ];
        assert_eq!(
            status_text(&off, Some(&findings), None),
            "Mod log: **off**\n\
             Give CrackTunes **View Audit Log** in Server Settings → Roles\n\
             Fetching is paused: CrackTunes couldn't read the audit log (it will try again in a few minutes or when settings are saved)\n\
             Anyone who can see <#55> can read the log."
        );
    }

    #[test]
    fn status_without_a_cache_answer_or_channel() {
        assert_eq!(
            status_text(&ModLogSettings::off(GuildId::new(1)), None, None),
            "Mod log: **off**\nCouldn't check permissions right now."
        );
    }
}
