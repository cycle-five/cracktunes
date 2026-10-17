//! `/rooms setup|off|status`: an admin turns voice rooms on or off and checks
//! that CrackTunes can run them. Spec 1.7.
//!
//! Settings change only through `rooms::ops::save_settings` (R4), the same
//! path the dashboard uses. Turning rooms off leaves live rooms running until
//! they empty.
use crate::messaging::courier;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::{
    ROOM_NO_DATABASE, ROOM_SETTINGS_INVALID, ROOM_SETTINGS_SAVE_FAILED,
};
use crate::messaging::render::Rendered;
use crate::rooms::card::RoomCard;
use crate::rooms::ops::{save_settings, SettingsSaveError};
use crate::rooms::preflight;
use crate::rooms::settings::{RoomSettings, SettingsError};
use crate::{Context, Error};
use serenity::all::{GenericChannelId, GuildId};

/// Set up, turn off, or check voice rooms.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Rooms",
    slash_command,
    guild_only,
    default_member_permissions = "MANAGE_GUILD",
    required_permissions = "MANAGE_GUILD",
    subcommands("setup", "off", "status"),
    subcommand_required
)]
pub async fn rooms(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// The dashboard's settings page for `guild`, when its origin is known.
pub(crate) fn dashboard_link(origin: Option<&str>, guild: GuildId) -> Option<String> {
    let origin = origin?.trim().trim_end_matches('/');
    (!origin.is_empty()).then(|| format!("{origin}/g/{guild}/settings"))
}

/// What to tell an admin whose settings were not saved.
pub(crate) fn save_error_text(err: SettingsSaveError) -> &'static str {
    match err {
        SettingsSaveError::Invalid(SettingsError::NoDatabase) => ROOM_NO_DATABASE,
        SettingsSaveError::Invalid(_) => ROOM_SETTINGS_INVALID,
        SettingsSaveError::Db => ROOM_SETTINGS_SAVE_FAILED,
    }
}

/// The `Status` card for `guild`, as it is now.
#[cfg(not(tarpaulin_include))]
fn status_card(ctx: Context<'_>, guild: GuildId) -> RoomCard {
    let settings = ctx.data().rooms.settings(guild);
    let findings = settings
        .hub
        .and_then(|hub| preflight::for_guild(&ctx.serenity_context().cache, guild, hub));
    RoomCard::Status {
        settings,
        findings,
        dashboard: dashboard_link(std::env::var("WEB_PUBLIC_ORIGIN").ok().as_deref(), guild),
    }
}

#[cfg(not(tarpaulin_include))]
async fn say(ctx: Context<'_>, card: RoomCard) -> Result<(), Error> {
    courier::reply_as(ctx, CrackedMessage::Room(Box::new(card)), true).await?;
    Ok(())
}

#[cfg(not(tarpaulin_include))]
async fn say_text(ctx: Context<'_>, text: &'static str) -> Result<(), Error> {
    courier::reply_rendered(ctx, Rendered::text(text), true).await?;
    Ok(())
}

/// Turn voice rooms on, with a hub channel to join.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn setup(
    ctx: Context<'_>,
    #[description = "The voice channel people join to get a room"]
    #[channel_types("Voice")]
    hub: GenericChannelId,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    let settings = RoomSettings {
        enabled: true,
        hub: Some(hub.expect_channel()),
        ..ctx.data().rooms.settings(guild)
    };
    match save_settings(&ctx.data(), settings).await {
        Ok(()) => say(ctx, status_card(ctx, guild)).await,
        Err(err) => say_text(ctx, save_error_text(err)).await,
    }
}

/// Turn voice rooms off. Rooms that exist stay until they empty.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn off(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    let settings = RoomSettings {
        enabled: false,
        ..ctx.data().rooms.settings(guild)
    };
    match save_settings(&ctx.data(), settings).await {
        Ok(()) => say(ctx, RoomCard::Switched { on: false }).await,
        Err(err) => say_text(ctx, save_error_text(err)).await,
    }
}

/// Show the room settings and what CrackTunes still needs.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    say(ctx, status_card(ctx, guild)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude::all::{ChannelType, Permissions};

    #[test]
    fn rooms_is_manage_guild_only() {
        let command = rooms();
        assert_eq!(command.name, "rooms");
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
                .any(|c| c.name == "rooms"),
            "registered"
        );
        let mut subs: Vec<_> = command
            .subcommands
            .iter()
            .map(|s| s.name.as_ref())
            .collect();
        subs.sort();
        assert_eq!(subs, ["off", "setup", "status"]);
        assert!(command
            .subcommands
            .iter()
            .all(|s| s.required_permissions.contains(Permissions::MANAGE_GUILD)));
    }

    #[test]
    fn setup_hub_is_a_required_voice_channel_option() {
        let command = rooms();
        let setup = command
            .subcommands
            .iter()
            .find(|s| s.name == "setup")
            .unwrap();
        let hub = setup.parameters.iter().find(|p| p.name == "hub").unwrap();
        assert!(hub.required);
        assert_eq!(
            hub.channel_types.as_deref(),
            Some(&[ChannelType::Voice][..])
        );
    }

    #[test]
    fn dashboard_link_trims_the_slash_and_needs_an_origin() {
        let g = GuildId::new(42);
        assert_eq!(
            dashboard_link(Some("https://dash.example/"), g).as_deref(),
            Some("https://dash.example/g/42/settings")
        );
        assert_eq!(
            dashboard_link(Some(" https://dash.example "), g).as_deref(),
            Some("https://dash.example/g/42/settings")
        );
        assert_eq!(dashboard_link(Some(""), g), None);
        assert_eq!(dashboard_link(Some("/"), g), None);
        assert_eq!(dashboard_link(None, g), None);
    }

    #[test]
    fn save_errors_have_their_own_words() {
        assert_eq!(
            save_error_text(SettingsSaveError::Invalid(SettingsError::NoDatabase)),
            ROOM_NO_DATABASE
        );
        assert_eq!(
            save_error_text(SettingsSaveError::Invalid(SettingsError::BadGrace)),
            ROOM_SETTINGS_INVALID
        );
        assert_eq!(
            save_error_text(SettingsSaveError::Db),
            ROOM_SETTINGS_SAVE_FAILED
        );
    }
}
