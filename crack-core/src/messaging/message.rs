use crack_types::DICE_ROLL;
use std::{borrow::Cow, fmt::Display};

use ::serenity::{builder::CreateEmbed, small_fixed_array::FixedString};
#[cfg(feature = "crack-osint")]
use crack_osint::virustotal::VirusTotalApiResponse;
use crack_types::messages::SCAN_QUEUED;
use poise::serenity_prelude as serenity;
use serenity::{Mention, Mentionable, UserId};
use songbird::error::ControlError;
use std::time::Duration;

//use crate::{errors::CrackedError, messaging::messages::*, utils::duration_to_string};
use crate::utils::duration_to_string;
use crack_types::{
    errors::CrackedError,
    messaging::messages::{
        ADDED_QUEUE, AUTHORIZED, AUTOPAUSE_OFF, AUTOPAUSE_ON, AUTOPLAY_OFF, AUTOPLAY_ON, AUTO_ROLE,
        BANNED, BUG, BUG_END, CATEGORY_CREATED, CHANNEL_DELETED, CHANNEL_SIZE_SET, CLEANED,
        CLEARED, COINFLIP, DEAFENED, DEAFENED_FAIL, DEAUTHORIZED, ERROR, FAIL_INVALID_IP,
        GRABBED_NOTICE, INVITE_LINK_TEXT, INVITE_TEXT, INVITE_URL, IP_DETAILS, JOINING, KICKED,
        LEAVING, LOOP_DISABLED, LOOP_ENABLED, MUTED, NO_AUTO_ROLE, OLD_VOLUME, ONETWOFT,
        OWNERS_ONLY, PAGINATION_COMPLETE, PASSWORD_PWNED, PASSWORD_SAFE, PAUSED,
        PHONE_NUMBER_INFO_ERROR, PLAYLIST_ADD_FAILURE, PLAYLIST_ADD_SUCCESS, PLAYLIST_CREATED,
        PLAYLIST_TRACKS, PLAY_ALL_FAILED, PLAY_FAILED_BLOCKED_DOMAIN, PLAY_LOG, PLAY_PLAYLIST,
        PLAY_QUEUING, PREFIXES, PREMIUM, PREMIUM_PLUG, QUEUE_NOW_PLAYING, REMOVED_QUEUE_MULTIPLE,
        RESUMED, ROLE_CREATED, ROLE_DELETED, ROLE_NOT_FOUND, SEARCHING, SEEKED, SEEK_FAIL,
        SETTINGS_RELOADED, SHUFFLED_SUCCESS, SKIPPED, SKIPPED_ALL, SKIPPED_TO, SKIP_VOTE_EMOJI,
        SKIP_VOTE_MISSING, SKIP_VOTE_USER, SONG_MOVED, SONG_MOVED_FROM, SONG_MOVED_TO, STOPPED,
        SUBCOMMAND_NOT_FOUND, TEXT_CHANNEL_CREATED, TIMEOUT, UNBANNED, UNDEAFENED, UNDEAFENED_FAIL,
        UNMUTED, UNTIL, VERSION, VERSION_LATEST, VERSION_LATEST_HASH, VOICE_CHANNEL_CREATED,
        VOLUME, VOTE_TOPGG_NOT_VOTED, VOTE_TOPGG_VOTED, WAYBACK_SNAPSHOT,
    },
};

pub const RELEASES_LINK: &str = "https://github.com/cycle-five/cracktunes/releases";
pub const REPO_LINK: &str = "https://github.com/cycle-five/cracktunes/";

#[repr(u8)]
#[derive(Debug)]
pub enum CrackedMessage {
    AutopauseOff,
    AutopauseOn,
    AutoplayOff,
    AutoplayOn,
    AutoRole(serenity::RoleId),
    BugNone(String),
    CategoryCreated {
        channel_id: serenity::ChannelId,
        channel_name: String,
    },
    CountryName(String),
    Coinflip(bool),
    ChannelSizeSet {
        id: serenity::ChannelId,
        name: String,
        size: u32,
    },
    ChannelDeleted {
        channel_id: serenity::ChannelId,
        channel_name: String,
    },
    Clear,
    Clean(i32),
    CrackedError(CrackedError),
    CrackedRed(String),
    CreateEmbed(Box<CreateEmbed<'static>>),
    CommandFound(String),
    DiceRoll {
        dice: u32,
        sides: u32,
        results: Vec<u32>,
    },
    DomainInfo(String),
    Error,
    ErrorHttp(serenity::http::HttpError),
    GrabbedNotice,
    InvalidIP(String),
    InviteLink,
    IPDetails(String),
    IPVersion(String),
    Leaving,
    LoopDisable,
    LoopEnable,
    NoAutoRole,
    NowPlaying,
    Other(String),
    OwnersOnly,
    PaginationComplete,
    Pause,
    PasswordPwned,
    PasswordSafe,
    Paywall(String),
    PhoneNumberInfo(String),
    PhoneNumberInfoError,
    PlayAllFailed,
    PlayDomainBanned {
        domain: String,
    },
    PlaylistAddSuccess {
        track: String,
        playlist: String,
    },
    PlaylistAddFailure {
        track: String,
        playlist: String,
    },
    PlaylistCreated(String, usize),
    PlaylistQueued,
    PlaylistQueuing(String),
    PlayLog(Vec<String>),
    Pong,
    Prefixes(Vec<String>),
    Premium(bool),
    PremiumPlug,
    RemoveMultiple,
    Resume,
    RoleCreated {
        role_id: serenity::RoleId,
        role_name: FixedString,
    },
    RoleDeleted {
        role_id: serenity::RoleId,
        role_name: FixedString,
    },
    RoleNotFound,
    #[cfg(feature = "crack-osint")]
    ScanResult {
        result: VirusTotalApiResponse,
    },
    #[cfg(feature = "crack-osint")]
    ScanResultQueued {
        id: String,
    },
    Search,
    Seek {
        timestamp: String,
    },
    SeekFail {
        timestamp: Cow<'static, String>,
        error: ControlError,
    },
    SettingsReload,
    Shuffle,
    Skip,
    SkipAll,
    SkipTo {
        title: String,
        url: String,
    },
    Stop,
    SubcommandNotFound {
        group: Cow<'static, String>,
        subcommand: Cow<'static, String>,
    },
    SocialMediaResponse {
        response: String,
    },
    SongMoved {
        at: usize,
        to: usize,
    },
    SongQueued {
        title: String,
        url: String,
    },
    Summon {
        mention: Mention,
    },
    TextChannelCreated {
        channel_id: serenity::ChannelId,
        channel_name: FixedString<u16>,
    },
    Uptime {
        mention: String,
        seconds: u64,
    },
    UserAuthorized {
        id: UserId,
        mention: Mention,
        guild_id: serenity::GuildId,
        guild_name: FixedString,
    },
    UserDeauthorized {
        id: UserId,
        mention: Mention,
        guild_id: serenity::GuildId,
        guild_name: FixedString,
    },
    UserTimeout {
        id: UserId,
        mention: Mention,
        timeout_until: FixedString,
    },
    UserKicked {
        mention: Mention,
        id: UserId,
    },
    UserBanned {
        mention: Mention,
        id: UserId,
    },
    UserUnbanned {
        mention: Mention,
        id: UserId,
    },
    UserMuted {
        mention: Mention,
        id: UserId,
    },
    UserUnmuted {
        mention: Mention,
        id: UserId,
    },
    UserDeafened {
        mention: Mention,
        id: UserId,
    },
    UserDeafenedFail {
        mention: Mention,
        id: UserId,
    },
    UserUndeafened {
        mention: Mention,
        id: UserId,
    },
    UserUndeafenedFail {
        mention: Mention,
        id: UserId,
    },
    Version {
        current: String,
        hash: String,
    },
    VoteTopggVoted,
    VoteTopggNotVoted,
    VoteSkip {
        mention: Mention,
        missing: usize,
    },
    VoiceChannelCreated {
        channel_name: String,
    },
    Volume {
        vol: f32,
        old_vol: f32,
    },
    WaybackSnapshot {
        url: String,
    },
    WelcomeSettings(String),
}

impl CrackedMessage {
    fn discriminant(&self) -> u8 {
        unsafe { *std::ptr::from_ref::<Self>(self).cast::<u8>() }
    }
}

impl PartialEq for CrackedMessage {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant() == other.discriminant()
    }
}

impl Display for CrackedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AutoplayOff => f.write_str(AUTOPLAY_OFF),
            Self::AutoplayOn => f.write_str(AUTOPLAY_ON),
            Self::AutoRole(role_id) => f.write_str(&format!("{} {}", AUTO_ROLE, role_id.mention())),
            Self::BugNone(variable) => f.write_str(&format!("{BUG} {variable} {BUG_END}")),
            Self::InvalidIP(ip) => f.write_str(&format!("{ip} {FAIL_INVALID_IP}")),
            Self::InviteLink => {
                f.write_str(&format!("{INVITE_TEXT} [{INVITE_LINK_TEXT}]({INVITE_URL})",))
            },
            Self::IPDetails(ip) => f.write_str(&format!("{IP_DETAILS} **{ip}**")),
            Self::IPVersion(ipv) => f.write_str(&format!("**{ipv}**")),
            Self::AutopauseOff => f.write_str(AUTOPAUSE_OFF),
            Self::AutopauseOn => f.write_str(AUTOPAUSE_ON),
            Self::CountryName(name) => f.write_str(name),
            Self::Coinflip(heads) => f.write_str(&format!("{COINFLIP} {heads}")),
            Self::Clear => f.write_str(CLEARED),
            Self::Clean(n) => f.write_str(&format!("{CLEANED} {n}!")),
            Self::ChannelSizeSet { id, name, size } => {
                f.write_str(&format!("{CHANNEL_SIZE_SET} {name} {id} {size}"))
            },
            Self::ChannelDeleted {
                channel_id,
                channel_name,
            } => f.write_str(&format!("{CHANNEL_DELETED} {channel_id} {channel_name}",)),
            Self::CrackedError(err) => f.write_str(&format!("{err}")),
            Self::CrackedRed(s) => f.write_str(s),
            Self::CreateEmbed(embed) => f.write_str(&format!("{embed:#?}")),
            Self::CommandFound(s) => f.write_str(s),
            Self::DomainInfo(info) => f.write_str(info),
            Self::DiceRoll {
                dice,
                sides,
                results,
            } => f.write_str(DICE_ROLL!(dice, sides, results)),
            Self::Error => f.write_str(ERROR),
            Self::ErrorHttp(err) => f.write_str(&format!("{err}")),
            Self::GrabbedNotice => f.write_str(GRABBED_NOTICE),
            Self::Leaving => f.write_str(LEAVING),
            Self::LoopDisable => f.write_str(LOOP_DISABLED),
            Self::LoopEnable => f.write_str(LOOP_ENABLED),
            Self::NoAutoRole => f.write_str(NO_AUTO_ROLE),
            Self::NowPlaying => f.write_str(QUEUE_NOW_PLAYING),
            Self::Other(message) => f.write_str(message),
            Self::OwnersOnly => f.write_str(OWNERS_ONLY),
            Self::PaginationComplete => f.write_str(PAGINATION_COMPLETE),
            Self::PasswordPwned => f.write_str(PASSWORD_PWNED),
            Self::PasswordSafe => f.write_str(PASSWORD_SAFE),
            Self::Pause => f.write_str(PAUSED),
            Self::Paywall(url) => f.write_str(&format!("{ONETWOFT}{url}")),
            Self::PhoneNumberInfo(info) => f.write_str(info),
            Self::PhoneNumberInfoError => f.write_str(PHONE_NUMBER_INFO_ERROR),
            Self::PlaylistAddSuccess { track, playlist } => {
                f.write_str(&format!("{PLAYLIST_ADD_SUCCESS} **{track}** {playlist}"))
            },
            Self::PlaylistAddFailure { track, playlist } => {
                f.write_str(&format!("{PLAYLIST_ADD_FAILURE} **{track}** {playlist}"))
            },
            Self::PlaylistCreated(name, len) => f.write_str(&format!(
                "{PLAYLIST_CREATED} **{name}**\n{PLAYLIST_TRACKS}: {len}!",
            )),
            Self::PlaylistQueuing(name) => f.write_str(&format!("{PLAY_QUEUING} **{name}**")),
            Self::PlaylistQueued => f.write_str(PLAY_PLAYLIST),
            Self::PlayAllFailed => f.write_str(PLAY_ALL_FAILED),
            Self::PlayDomainBanned { domain } => {
                f.write_str(&format!("⚠️ **{domain}** {PLAY_FAILED_BLOCKED_DOMAIN}"))
            },
            Self::PlayLog(log) => f.write_str(&format!("{PLAY_LOG}\n{log}", log = log.join("\n"))),
            Self::Pong => f.write_str("Pong"),
            Self::Prefixes(prefixes) => {
                f.write_str(&format!("{PREFIXES} {val}", val = prefixes.join(", ")))
            },
            Self::Premium(premium) => f.write_str(&format!("{PREMIUM} {premium}")),
            Self::PremiumPlug => f.write_str(PREMIUM_PLUG),
            #[cfg(feature = "crack-osint")]
            Self::ScanResult { result } => {
                f.write_str(&format!("{}", result.data.attributes.stats))
            },
            #[cfg(feature = "crack-osint")]
            Self::ScanResultQueued { id } => f.write_str(&format!("{SCAN_QUEUED} {id}")),
            Self::Search => f.write_str(SEARCHING),

            Self::RemoveMultiple => f.write_str(REMOVED_QUEUE_MULTIPLE),
            Self::Resume => f.write_str(RESUMED),
            Self::RoleCreated { role_id, role_name } => {
                f.write_str(&format!("{ROLE_CREATED} {role_id} {role_name}"))
            },
            Self::RoleDeleted { role_id, role_name } => {
                f.write_str(&format!("{ROLE_DELETED} {role_id} {role_name}"))
            },
            Self::RoleNotFound => f.write_str(ROLE_NOT_FOUND),
            Self::Shuffle => f.write_str(SHUFFLED_SUCCESS),
            Self::Stop => f.write_str(STOPPED),
            #[allow(clippy::literal_string_with_formatting_args)]
            Self::SubcommandNotFound { group, subcommand } => f.write_str(
                &SUBCOMMAND_NOT_FOUND
                    .replace("{group}", group)
                    .replace("{subcommand}", subcommand),
            ),
            Self::SettingsReload => f.write_str(SETTINGS_RELOADED),
            Self::VoteSkip { mention, missing } => f.write_str(&format!(
                "{SKIP_VOTE_EMOJI}{mention} {SKIP_VOTE_USER} {missing} {SKIP_VOTE_MISSING}",
            )),
            Self::SocialMediaResponse { response } => f.write_str(response),
            Self::SongMoved { at, to } => f.write_str(&format!(
                "{SONG_MOVED} {SONG_MOVED_FROM} {SONG_MOVED_TO} {at} {to}.",
            )),
            Self::SongQueued { title, url } => {
                f.write_str(&format!("{ADDED_QUEUE} [**{title}**]({url})"))
            },
            Self::Seek { timestamp } => f.write_str(&format!("{SEEKED} **{timestamp}**!")),
            Self::SeekFail { timestamp, error } => {
                f.write_str(&format!("{SEEK_FAIL} **{timestamp}**!\n{error}",))
            },
            Self::Skip => f.write_str(SKIPPED),
            Self::SkipAll => f.write_str(SKIPPED_ALL),
            Self::SkipTo { title, url } => {
                f.write_str(&format!("{SKIPPED_TO} [**{title}**]({url})!"))
            },
            Self::Summon { mention } => f.write_str(&format!("{JOINING} **{mention}**!")),
            Self::TextChannelCreated {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{TEXT_CHANNEL_CREATED} {channel_id} {channel_name}",
            )),
            Self::CategoryCreated {
                channel_id,
                channel_name,
            } => f.write_str(&format!("{CATEGORY_CREATED} {channel_id} {channel_name}",)),
            Self::Uptime { mention, seconds } => f.write_str(&format!(
                "**{mention}**\n {duration}",
                duration = duration_to_string(Duration::from_secs(*seconds)),
            )),
            Self::UserAuthorized {
                id,
                mention,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{AUTHORIZED}\n {mention} ({id}) Guild: {guild_name} ({guild_id})",
            )),
            Self::UserDeauthorized {
                id,
                mention,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{DEAUTHORIZED}\n {mention} ({id}) \n {guild_name} ({guild_id})"
            )),
            Self::UserTimeout {
                mention,
                id,
                timeout_until,
            } => f.write_str(&format!(
                "{TIMEOUT}\n{mention} ({id})\n{UNTIL}: {timeout_until}"
            )),
            Self::UserKicked { mention, id } => f.write_str(&format!("{KICKED}\n{mention} ({id})")),
            Self::UserBanned { mention, id } => f.write_str(&format!("{BANNED}\n{mention} ({id})")),
            Self::UserUnbanned { mention, id } => {
                f.write_str(&format!("{UNBANNED}\n{mention} ({id})"))
            },
            Self::UserUndeafened { mention, id } => {
                f.write_str(&format!("{UNDEAFENED} {mention} {id}"))
            },
            Self::UserDeafened { mention, id } => {
                f.write_str(&format!("{DEAFENED}\n{mention}({id})"))
            },
            Self::UserDeafenedFail { mention, id } => {
                f.write_str(&format!("{DEAFENED_FAIL}\n{mention} ({id})"))
            },
            Self::UserUndeafenedFail { mention, id } => {
                f.write_str(&format!("{UNDEAFENED_FAIL}\n{mention} ({id})"))
            },
            Self::UserMuted { mention, id } => f.write_str(&format!("{MUTED}\n{mention} {id}")),
            Self::UserUnmuted { mention, id } => f.write_str(&format!("{UNMUTED}\n{mention} {id}")),
            Self::Version { current, hash } => f.write_str(&format!(
                "{VERSION} [{current}]({RELEASES_LINK}/tag/v{current})\n{VERSION_LATEST}({RELEASES_LINK}/latest)\n{VERSION_LATEST_HASH}({REPO_LINK}tree/{hash})",
            )),
            Self::VoiceChannelCreated { channel_name } => {
                f.write_str(&format!("{VOICE_CHANNEL_CREATED} {channel_name}"))
            },
            Self::VoteTopggVoted => f.write_str(VOTE_TOPGG_VOTED),
            Self::VoteTopggNotVoted => f.write_str(VOTE_TOPGG_NOT_VOTED),
            Self::Volume { vol, old_vol } => {
                f.write_str(&format!("{VOLUME}: {vol}\n{OLD_VOLUME}: {old_vol}"))
            },
            Self::WaybackSnapshot { url } => f.write_str(&format!("{WAYBACK_SNAPSHOT} {url}")),
            Self::WelcomeSettings(settings) => f.write_str(settings),
        }
    }
}

impl From<CrackedMessage> for Cow<'_, str> {
    fn from(message: CrackedMessage) -> Self {
        message.to_string().into()
    }
}

impl From<CrackedMessage> for String {
    fn from(message: CrackedMessage) -> Self {
        message.to_string()
    }
}

impl From<CrackedMessage> for CreateEmbed<'_> {
    fn from(message: CrackedMessage) -> Self {
        CreateEmbed::default().description(message.to_string())
    }
}

impl From<CrackedError> for CrackedMessage {
    fn from(error: CrackedError) -> Self {
        Self::CrackedError(error)
    }
}

impl From<serenity::http::HttpError> for CrackedMessage {
    fn from(error: serenity::http::HttpError) -> Self {
        Self::ErrorHttp(error)
    }
}

impl Default for CrackedMessage {
    fn default() -> Self {
        Self::Other("(default)".to_string())
    }
}

use colored::Color;
impl From<CrackedMessage> for Color {
    fn from(message: CrackedMessage) -> Color {
        match message {
            CrackedMessage::Error => Color::Red,
            CrackedMessage::ErrorHttp(_) => Color::Red,
            CrackedMessage::CrackedError(_) => Color::Red,
            CrackedMessage::CrackedRed(_) => Color::Red,
            CrackedMessage::Other(_) => Color::Yellow,
            _ => Color::Blue,
        }
    }
}

impl From<&CrackedMessage> for Color {
    fn from(message: &CrackedMessage) -> Color {
        match message {
            CrackedMessage::Error => Color::Red,
            CrackedMessage::ErrorHttp(_) => Color::Red,
            CrackedMessage::CrackedError(_) => Color::Red,
            CrackedMessage::CrackedRed(_) => Color::Red,
            CrackedMessage::Other(_) => Color::Yellow,
            _ => Color::Blue,
        }
    }
}

use serenity::Colour;
impl From<CrackedMessage> for Colour {
    fn from(message: CrackedMessage) -> Colour {
        match message {
            CrackedMessage::Error => Colour::RED,
            CrackedMessage::ErrorHttp(_) => Colour::RED,
            CrackedMessage::CrackedError(_) => Colour::RED,
            CrackedMessage::CrackedRed(_) => Colour::RED,
            CrackedMessage::Other(_) => Colour::GOLD,
            _ => Colour::BLUE,
        }
    }
}

impl From<&CrackedMessage> for Colour {
    fn from(message: &CrackedMessage) -> Colour {
        match message {
            CrackedMessage::Error => Colour::RED,
            CrackedMessage::ErrorHttp(_) => Colour::RED,
            CrackedMessage::CrackedError(_) => Colour::RED,
            CrackedMessage::CrackedRed(_) => Colour::RED,
            CrackedMessage::Other(_) => Colour::GOLD,
            _ => Colour::BLUE,
        }
    }
}

/// Convert a [`CrackedMessage`] into a [`CreateEmbed`].
impl<'a, 'b> From<&'a CrackedMessage> for Option<CreateEmbed<'b>> {
    fn from(message: &'a CrackedMessage) -> Option<CreateEmbed<'b>> {
        // Why did I do this?
        match message {
            CrackedMessage::CreateEmbed(embed) => Some(*embed.clone()),
            msg => Some(CreateEmbed::default().description(msg.to_string())),
        }
    }
}

impl From<CrackedMessage> for crate::CrackedHowResult<CrackedMessage> {
    fn from(msg: CrackedMessage) -> crate::CrackedHowResult<CrackedMessage> {
        crate::CrackedHowResult::Ok(msg)
    }
}

#[cfg(test)]
mod test {
    use super::CrackedMessage;
    use poise::serenity_prelude as serenity;

    #[test]
    fn test_discriminant() {
        let message = CrackedMessage::AutopauseOff;
        assert_eq!(message.discriminant(), 0);

        let message = CrackedMessage::AutopauseOn;
        assert_eq!(message.discriminant(), 1);

        let message = CrackedMessage::AutoplayOff;
        assert_eq!(message.discriminant(), 2);

        let message = CrackedMessage::AutoplayOn;
        assert_eq!(message.discriminant(), 3);

        let message = CrackedMessage::Clear;
        assert_eq!(message.discriminant(), 11);
    }

    #[test]
    fn test_eq() {
        let message = CrackedMessage::AutopauseOff;
        assert_eq!(message, CrackedMessage::AutopauseOff);

        let message = CrackedMessage::AutopauseOn;
        assert_eq!(message, CrackedMessage::AutopauseOn);

        let message = CrackedMessage::BugNone("test".to_string());
        assert_eq!(message, CrackedMessage::BugNone("test".to_string()));

        let message = CrackedMessage::InvalidIP("test".to_string());
        assert_eq!(message, CrackedMessage::InvalidIP("test".to_string()));

        let message = CrackedMessage::IPDetails("test".to_string());
        assert_eq!(message, CrackedMessage::IPDetails("test".to_string()));

        let message = CrackedMessage::IPVersion("test".to_string());
        assert_eq!(message, CrackedMessage::IPVersion("test".to_string()));

        let message = CrackedMessage::AutopauseOff;
        assert_eq!(message, CrackedMessage::AutopauseOff);

        let message = CrackedMessage::AutopauseOn;
        assert_eq!(message, CrackedMessage::AutopauseOn);

        let message = CrackedMessage::CountryName("test".to_string());
        assert_eq!(message, CrackedMessage::CountryName("test".to_string()));

        let message = CrackedMessage::Clear;
        assert_eq!(message, CrackedMessage::Clear);

        let message = CrackedMessage::Clean(1);
        assert_eq!(message, CrackedMessage::Clean(1));

        let message = CrackedMessage::ChannelSizeSet {
            id: serenity::ChannelId::default(),
            name: "test".to_string(),
            size: 1,
        };
        assert_eq!(
            message,
            CrackedMessage::ChannelSizeSet {
                id: serenity::ChannelId::default(),
                name: "test".to_string(),
                size: 1
            }
        );

        let message = CrackedMessage::ChannelDeleted {
            channel_id: serenity::ChannelId::default(),
            channel_name: "test".to_string(),
        };
        assert_eq!(
            message,
            CrackedMessage::ChannelDeleted {
                channel_id: serenity::ChannelId::default(),
                channel_name: "test".to_string()
            }
        );
    }

    #[test]
    fn test_ne() {
        let message = CrackedMessage::AutopauseOff;
        assert_ne!(message, CrackedMessage::AutopauseOn);

        let message = CrackedMessage::AutopauseOn;
        assert_ne!(message, CrackedMessage::AutopauseOff);

        let message = CrackedMessage::BugNone("test".to_string());
        assert_ne!(message, CrackedMessage::InvalidIP("test".to_string()));

        let message = CrackedMessage::InvalidIP("test".to_string());
        assert_ne!(message, CrackedMessage::BugNone("test".to_string()));

        let message = CrackedMessage::IPDetails("test".to_string());
        assert_ne!(message, CrackedMessage::IPVersion("test".to_string()));

        let message = CrackedMessage::IPVersion("test".to_string());
        assert_ne!(message, CrackedMessage::IPDetails("test".to_string()));

        let message = CrackedMessage::AutopauseOff;
        assert_ne!(message, CrackedMessage::AutopauseOn);

        let message = CrackedMessage::AutopauseOn;
        assert_ne!(message, CrackedMessage::AutopauseOff);

        let message = CrackedMessage::CountryName("test".to_string());
        assert_ne!(message, CrackedMessage::Clear);

        let message = CrackedMessage::Clear;
        assert_ne!(message, CrackedMessage::CountryName("test".to_string()));

        let message = CrackedMessage::Clean(1);
        assert_ne!(
            message,
            CrackedMessage::ChannelSizeSet {
                id: serenity::ChannelId::default(),
                name: "test".to_string(),
                size: 1,
            }
        );

        let message = CrackedMessage::ChannelSizeSet {
            id: serenity::ChannelId::default(),
            name: "test".to_string(),
            size: 1,
        };
        assert_ne!(message, CrackedMessage::Clean(1));

        let message = CrackedMessage::ChannelDeleted {
            channel_id: serenity::ChannelId::default(),
            channel_name: "test".to_string(),
        };
        assert_ne!(
            message,
            CrackedMessage::ChannelSizeSet {
                id: serenity::ChannelId::default(),
                name: "test".to_string(),
                size: 1,
            }
        );
    }
}
