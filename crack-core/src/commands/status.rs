//! `/status`: this server's plan, the bot, what's playing here, and the
//! settings in effect. Anyone in the server can ask; only they see the answer.

use crate::guild::operations::GuildSettingsOperations;
use crate::guild::plan::Plan;
use crate::messaging::messages::{
    STATUS_AUTOPAUSE, STATUS_AUTOPLAY, STATUS_BOT, STATUS_FREE, STATUS_GAME, STATUS_IDLE,
    STATUS_IDLE_TIMEOUT, STATUS_MORE_QUEUED, STATUS_NEVER_PREMIUM, STATUS_OFF, STATUS_ON,
    STATUS_PLAYBACK, STATUS_PREMIUM, STATUS_SERVER, STATUS_SETTINGS, STATUS_TITLE, STATUS_UNTITLED,
    STATUS_VOLUME,
};
use crate::music::audit_view::escape;
use crate::music::remote::{self, QueueState};
use crate::{Context, Error};
use poise::serenity_prelude as serenity;
use poise::CreateReply;
use serenity::{ChannelId, CreateEmbed, Mentionable, UserId};
use std::time::{Duration, SystemTime};

/// Titles longer than this are cut, with `…`.
const TITLE_MAX: usize = 60;

/// Show this server's plan, the bot, what's playing, and the settings in effect.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, guild_only, ephemeral)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(crate::CrackedError::NoGuildId)?;
    let data = ctx.data();
    // Read before any await: the cache guard is not `Send`.
    let bot = ctx.cache().current_user().id;
    let playback = match remote::queue_state(&data, guild_id).await {
        QueueState::Idle => Playback::Idle,
        QueueState::Hidden => Playback::Game,
        QueueState::Playing {
            bot_channel,
            tracks,
            ..
        } => {
            let now = tracks.first();
            Playback::Playing {
                channel: bot_channel,
                title: now.and_then(|t| t.title.clone()),
                duration: now.and_then(|t| t.duration),
                more: tracks.len().saturating_sub(1),
            }
        },
    };
    let facts = StatusFacts {
        plan: Plan::of(data.get_premium(guild_id).await),
        bot,
        version: env!("CARGO_PKG_VERSION"),
        uptime: SystemTime::now()
            .duration_since(data.start_time)
            .unwrap_or_default(),
        playback,
        timeout_secs: data.get_timeout(guild_id).await.unwrap_or(0),
        volume: data.get_volume(guild_id).await.0,
        autopause: data.get_autopause(guild_id).await,
        autoplay: data.get_autoplay(guild_id).await,
    };
    let embed = compose_status(&facts)
        .into_iter()
        .fold(CreateEmbed::default().title(STATUS_TITLE), |e, s| {
            e.field(s.name, s.value, false)
        });
    ctx.send(CreateReply::default().embed(embed).ephemeral(true))
        .await?;
    Ok(())
}

/// What is playing in this server, as `/status` may tell it.
#[derive(Debug, Clone, PartialEq)]
pub enum Playback {
    /// Not in voice here, or nothing queued.
    Idle,
    /// A `/gp` game owns playback: its titles are the answers, so none is told.
    Game,
    Playing {
        channel: ChannelId,
        title: Option<String>,
        duration: Option<Duration>,
        /// Tracks queued after the one playing.
        more: usize,
    },
}

/// Everything `/status` reports, gathered.
#[derive(Debug, Clone)]
pub struct StatusFacts {
    pub plan: Plan,
    pub bot: UserId,
    pub version: &'static str,
    pub uptime: Duration,
    pub playback: Playback,
    /// The idle timeout in seconds; 0 is off.
    pub timeout_secs: u32,
    /// 1.0 is 100%, as `/volume` stores it.
    pub volume: f32,
    pub autopause: bool,
    /// For this session, which is what's in effect.
    pub autoplay: bool,
}

/// One embed field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSection {
    pub name: &'static str,
    pub value: String,
}

/// The four sections `/status` shows: server, bot, playback, settings.
#[must_use]
pub fn compose_status(f: &StatusFacts) -> Vec<StatusSection> {
    let server = match f.plan {
        Plan::Premium => STATUS_PREMIUM.to_owned(),
        Plan::Free => STATUS_FREE.to_owned(),
    };
    let bot = format!(
        "{} v{}, up {}",
        f.bot.mention(),
        f.version,
        uptime_text(f.uptime)
    );
    let playback = match &f.playback {
        Playback::Idle => STATUS_IDLE.to_owned(),
        Playback::Game => STATUS_GAME.to_owned(),
        Playback::Playing {
            channel,
            title,
            duration,
            more,
        } => {
            let mut line = format!(
                "In {}, playing **{}**",
                channel.mention(),
                title_text(title)
            );
            if let Some(d) = duration {
                line.push_str(&format!(" ({})", clock(*d)));
            }
            line.push('.');
            if *more > 0 {
                line.push_str(&format!(" {more} {STATUS_MORE_QUEUED}"));
            }
            line
        },
    };
    let idle = match (f.plan, f.timeout_secs) {
        (Plan::Premium, _) => STATUS_NEVER_PREMIUM.to_owned(),
        (Plan::Free, 0) => STATUS_OFF.to_owned(),
        (Plan::Free, s) => minutes_text(s),
    };
    let on_off = |b: bool| if b { STATUS_ON } else { STATUS_OFF };
    let settings = format!(
        "{STATUS_IDLE_TIMEOUT}: {idle}\n{STATUS_VOLUME}: {:.0}%\n{STATUS_AUTOPAUSE}: {}\n{STATUS_AUTOPLAY}: {}",
        f.volume * 100.0,
        on_off(f.autopause),
        on_off(f.autoplay),
    );
    vec![
        StatusSection {
            name: STATUS_SERVER,
            value: server,
        },
        StatusSection {
            name: STATUS_BOT,
            value: bot,
        },
        StatusSection {
            name: STATUS_PLAYBACK,
            value: playback,
        },
        StatusSection {
            name: STATUS_SETTINGS,
            value: settings,
        },
    ]
}

/// A title for display: escaped, cut at `TITLE_MAX` characters with `…`.
fn title_text(title: &Option<String>) -> String {
    let Some(raw) = title.as_deref() else {
        return STATUS_UNTITLED.to_owned();
    };
    let cut: String = raw.chars().take(TITLE_MAX).collect();
    let cut = if raw.chars().count() > TITLE_MAX {
        format!("{cut}…")
    } else {
        cut
    };
    escape(&cut)
}

/// `3:12`, or `1:03:12` past an hour.
fn clock(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `3 d 4 h`, `4 h 12 min`, `12 min`, or `under a minute`.
fn uptime_text(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins) = (s / 86_400, (s / 3600) % 24, (s / 60) % 60);
    match (days, hours, mins) {
        (0, 0, 0) => "under a minute".to_owned(),
        (0, 0, m) => format!("{m} min"),
        (0, h, m) => format!("{h} h {m} min"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

/// An idle timeout: `10 min`, or `45 s` under a minute.
fn minutes_text(secs: u32) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min", secs / 60)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn facts(plan: Plan, playback: Playback) -> StatusFacts {
        StatusFacts {
            plan,
            bot: UserId::new(1115229568006103122),
            version: "0.20.0",
            uptime: Duration::from_secs(3 * 3600 + 12 * 60),
            playback,
            timeout_secs: 600,
            volume: 0.5,
            autopause: true,
            autoplay: false,
        }
    }

    fn section(f: &StatusFacts, name: &str) -> String {
        compose_status(f)
            .into_iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no {name} section"))
            .value
    }

    #[test]
    fn four_sections_in_order() {
        let names: Vec<&str> = compose_status(&facts(Plan::Free, Playback::Idle))
            .iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            names,
            vec![STATUS_SERVER, STATUS_BOT, STATUS_PLAYBACK, STATUS_SETTINGS]
        );
    }

    #[test]
    fn the_server_says_its_plan() {
        assert_eq!(
            section(&facts(Plan::Premium, Playback::Idle), STATUS_SERVER),
            STATUS_PREMIUM
        );
        assert_eq!(
            section(&facts(Plan::Free, Playback::Idle), STATUS_SERVER),
            STATUS_FREE
        );
    }

    #[test]
    fn the_bot_says_who_what_version_and_how_long() {
        assert_eq!(
            section(&facts(Plan::Free, Playback::Idle), STATUS_BOT),
            "<@1115229568006103122> v0.20.0, up 3 h 12 min"
        );
    }

    #[test]
    fn idle_says_idle() {
        assert_eq!(
            section(&facts(Plan::Free, Playback::Idle), STATUS_PLAYBACK),
            STATUS_IDLE
        );
    }

    #[test]
    fn playing_says_where_what_how_long_and_what_is_next() {
        let f = facts(
            Plan::Free,
            Playback::Playing {
                channel: ChannelId::new(42),
                title: Some("Song A".to_owned()),
                duration: Some(Duration::from_secs(192)),
                more: 4,
            },
        );
        assert_eq!(
            section(&f, STATUS_PLAYBACK),
            format!("In <#42>, playing **Song A** (3:12). 4 {STATUS_MORE_QUEUED}")
        );
    }

    #[test]
    fn a_hostile_title_is_escaped_and_long_ones_are_cut() {
        let f = facts(
            Plan::Free,
            Playback::Playing {
                channel: ChannelId::new(42),
                title: Some("**[x](http://e.vil)** <@&1>".to_owned()),
                duration: None,
                more: 0,
            },
        );
        let line = section(&f, STATUS_PLAYBACK);
        assert!(
            line.contains(r"\*\*\[x\](http://e.vil)\*\* \<@&1\>"),
            "{line}"
        );
        assert!(!line.contains("more"), "{line}");

        let long = "a".repeat(TITLE_MAX + 5);
        let f = facts(
            Plan::Free,
            Playback::Playing {
                channel: ChannelId::new(42),
                title: Some(long),
                duration: None,
                more: 0,
            },
        );
        assert!(section(&f, STATUS_PLAYBACK).contains(&format!("{}…", "a".repeat(TITLE_MAX))));
    }

    /// The titles in a `/gp` game are its answers.
    #[test]
    fn a_game_tells_no_titles() {
        assert_eq!(
            section(&facts(Plan::Free, Playback::Game), STATUS_PLAYBACK),
            STATUS_GAME
        );
    }

    #[test]
    fn settings_read_as_they_are_in_effect() {
        assert_eq!(
            section(&facts(Plan::Free, Playback::Idle), STATUS_SETTINGS),
            format!(
                "{STATUS_IDLE_TIMEOUT}: 10 min\n{STATUS_VOLUME}: 50%\n{STATUS_AUTOPAUSE}: {STATUS_ON}\n{STATUS_AUTOPLAY}: {STATUS_OFF}"
            )
        );
    }

    /// Premium servers never time out, whatever the setting says.
    #[test]
    fn premium_never_times_out_and_zero_is_off() {
        let premium = section(&facts(Plan::Premium, Playback::Idle), STATUS_SETTINGS);
        assert!(
            premium.starts_with(&format!("{STATUS_IDLE_TIMEOUT}: {STATUS_NEVER_PREMIUM}\n")),
            "{premium}"
        );
        let mut off = facts(Plan::Free, Playback::Idle);
        off.timeout_secs = 0;
        assert!(section(&off, STATUS_SETTINGS)
            .starts_with(&format!("{STATUS_IDLE_TIMEOUT}: {STATUS_OFF}\n")));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(clock(Duration::from_secs(192)), "3:12");
        assert_eq!(clock(Duration::from_secs(3792)), "1:03:12");
        assert_eq!(uptime_text(Duration::from_secs(30)), "under a minute");
        assert_eq!(uptime_text(Duration::from_secs(12 * 60)), "12 min");
        assert_eq!(
            uptime_text(Duration::from_secs(3 * 86_400 + 4 * 3600)),
            "3 d 4 h"
        );
        assert_eq!(minutes_text(45), "45 s");
        assert_eq!(minutes_text(600), "10 min");
    }

    #[test]
    fn status_is_for_anyone_in_a_server() {
        let cmd = status();
        assert!(cmd.guild_only);
        assert!(!cmd.owners_only);
        assert!(cmd.required_permissions.is_empty());
        assert!(cmd.default_member_permissions.is_empty());
        assert!(crate::commands::all_command_names()
            .iter()
            .any(|n| n == "status"));
    }
}
