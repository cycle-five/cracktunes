use std::{borrow::Cow, fmt::Display};

use ::serenity::{builder::CreateEmbed, small_fixed_array::FixedString};
#[cfg(feature = "crack-osint")]
use crack_osint::virustotal::VirusTotalApiResponse;
use poise::serenity_prelude as serenity;
use serenity::{Mention, Mentionable, UserId};
use songbird::error::ControlError;
use std::time::Duration;

use crate::{errors::CrackedError, messaging::messages::*, utils::duration_to_string};

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
        channel_id: serenity::GenericChannelId,
        channel_name: String,
    },
    CountryName(String),
    Coinflip(bool),
    ChannelSizeSet {
        id: serenity::GenericChannelId,
        name: String,
        size: u32,
    },
    ChannelDeleted {
        channel_id: serenity::GenericChannelId,
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
        channel_id: serenity::GenericChannelId,
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
    // Guilty pleasure game (`commands::music::gp`). Appended last so the
    // discriminants of the variants above are unchanged.
    GpSubmitted {
        title: String,
        replaced: bool,
        submitted: usize,
        of: usize,
    },
    GpStarted {
        category: &'static str,
        rounds: usize,
        timer_secs: u64,
        cleared_queue: bool,
    },
    GpRoundSkipped,
    GpEnded {
        by: String,
    },
    GpWindowClosed {
        count: usize,
    },
}

impl CrackedMessage {
    fn discriminant(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
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
            Self::BugNone(variable) => f.write_str(&format!("{} {} {}", BUG, variable, BUG_END)),
            Self::InvalidIP(ip) => f.write_str(&format!("{} {}", ip, FAIL_INVALID_IP)),
            Self::InviteLink => f.write_str(&format!(
                "{} [{}]({})",
                INVITE_TEXT, INVITE_LINK_TEXT, INVITE_URL
            )),
            Self::IPDetails(ip) => f.write_str(&format!("{} **{}**", IP_DETAILS, ip)),
            Self::IPVersion(ipv) => f.write_str(&format!("**{}**", ipv)),
            Self::AutopauseOff => f.write_str(AUTOPAUSE_OFF),
            Self::AutopauseOn => f.write_str(AUTOPAUSE_ON),
            Self::CountryName(name) => f.write_str(name),
            Self::Coinflip(heads) => f.write_str(&format!("{} {}", COINFLIP, heads)),
            Self::Clear => f.write_str(CLEARED),
            Self::Clean(n) => f.write_str(&format!("{} {}!", CLEANED, n)),
            Self::ChannelSizeSet { id, name, size } => {
                f.write_str(&format!("{} {} {} {}", CHANNEL_SIZE_SET, name, id, size))
            },
            Self::ChannelDeleted {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{} {} {}",
                CHANNEL_DELETED, channel_id, channel_name
            )),
            Self::CrackedError(err) => f.write_str(&format!("{}", err)),
            Self::CrackedRed(s) => f.write_str(s),
            Self::CreateEmbed(embed) => f.write_str(&format!("{:#?}", embed)),
            Self::CommandFound(s) => f.write_str(s),
            Self::DomainInfo(info) => f.write_str(info),
            Self::DiceRoll {
                dice,
                sides,
                results,
            } => f.write_str(crate::DICE_ROLL!(dice, sides, results)),
            Self::Error => f.write_str(ERROR),
            Self::ErrorHttp(err) => f.write_str(&format!("{}", err)),
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
            Self::Paywall(url) => f.write_str(&format!("{}{}", ONETWOFT, url)),
            Self::PhoneNumberInfo(info) => f.write_str(info),
            Self::PhoneNumberInfoError => f.write_str(PHONE_NUMBER_INFO_ERROR),
            Self::PlaylistCreated(name, len) => f.write_str(&format!(
                "{} **{}** with {} tracks!",
                PLAYLIST_CREATED, name, len
            )),
            Self::PlaylistQueuing(name) => f.write_str(&format!("Queuing **{}**", name)),
            Self::PlaylistQueued => f.write_str(PLAY_PLAYLIST),
            Self::PlayAllFailed => f.write_str(PLAY_ALL_FAILED),
            Self::PlayDomainBanned { domain } => {
                f.write_str(&format!("⚠️ **{}** {}", domain, PLAY_FAILED_BLOCKED_DOMAIN))
            },
            Self::PlayLog(log) => f.write_str(&format!("{}\n{}", PLAY_LOG, log.join("\n"))),
            Self::Pong => f.write_str("Pong"),
            Self::Prefixes(prefixes) => {
                f.write_str(&format!("{} {}", PREFIXES, prefixes.join(", ")))
            },
            Self::Premium(premium) => f.write_str(&format!("{} {}", PREMIUM, premium)),
            Self::PremiumPlug => f.write_str(PREMIUM_PLUG),
            #[cfg(feature = "crack-osint")]
            Self::ScanResult { result } => {
                f.write_str(&format!("{}", result.data.attributes.stats))
            },
            #[cfg(feature = "crack-osint")]
            Self::ScanResultQueued { id } => f.write_str(&format!("{} {}", SCAN_QUEUED, id)),
            Self::Search => f.write_str(SEARCHING),

            Self::RemoveMultiple => f.write_str(REMOVED_QUEUE_MULTIPLE),
            Self::Resume => f.write_str(RESUMED),
            Self::RoleCreated { role_id, role_name } => {
                f.write_str(&format!("{} {} {}", ROLE_CREATED, role_id, role_name))
            },
            Self::RoleDeleted { role_id, role_name } => {
                f.write_str(&format!("{} {} {}", ROLE_DELETED, role_id, role_name))
            },
            Self::RoleNotFound => f.write_str(ROLE_NOT_FOUND),
            Self::Shuffle => f.write_str(SHUFFLED_SUCCESS),
            Self::Stop => f.write_str(STOPPED),
            Self::SubcommandNotFound { group, subcommand } => f.write_str(
                &SUBCOMMAND_NOT_FOUND
                    .replace("{group}", group)
                    .replace("{subcommand}", subcommand),
            ),
            Self::VoteSkip { mention, missing } => f.write_str(&format!(
                "{}{} {} {} {}",
                SKIP_VOTE_EMOJI, mention, SKIP_VOTE_USER, missing, SKIP_VOTE_MISSING
            )),
            Self::SocialMediaResponse { response } => f.write_str(response),
            Self::SongMoved { at, to } => f.write_str(&format!(
                "{} {} {} {} {}.",
                SONG_MOVED, SONG_MOVED_FROM, SONG_MOVED_TO, at, to
            )),
            Self::SongQueued { title, url } => {
                f.write_str(&format!("{} [**{}**]({})", ADDED_QUEUE, title, url))
            },
            Self::Seek { timestamp } => f.write_str(&format!("{} **{}**!", SEEKED, timestamp)),
            Self::SeekFail { timestamp, error } => {
                f.write_str(&format!("{} **{}**!\n{}", SEEK_FAIL, timestamp, error))
            },
            Self::Skip => f.write_str(SKIPPED),
            Self::SkipAll => f.write_str(SKIPPED_ALL),
            Self::SkipTo { title, url } => {
                f.write_str(&format!("{} [**{}**]({})!", SKIPPED_TO, title, url))
            },
            Self::Summon { mention } => f.write_str(&format!("{} **{}**!", JOINING, mention)),
            Self::TextChannelCreated {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{} {} {}",
                TEXT_CHANNEL_CREATED, channel_id, channel_name
            )),
            Self::CategoryCreated {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{} {} {}",
                CATEGORY_CREATED, channel_id, channel_name
            )),
            Self::Uptime { mention, seconds } => f.write_str(&format!(
                "**{}**\n {}",
                mention,
                duration_to_string(Duration::from_secs(*seconds)),
            )),
            Self::UserAuthorized {
                id,
                mention,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{}\n User: {} ({}) Guild: {} ({})",
                AUTHORIZED, mention, id, guild_name, guild_id
            )),
            Self::UserDeauthorized {
                id,
                mention,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{}\n User: {} ({}) Guild: {} ({})",
                DEAUTHORIZED, mention, id, guild_name, guild_id
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
                f.write_str(&format!("{} {} {}", UNDEAFENED, mention, id))
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
                "{} [{}]({}/tag/v{})\n{}({}/latest)\n{}({}tree/{})",
                VERSION,
                current,
                RELEASES_LINK,
                current,
                VERSION_LATEST,
                RELEASES_LINK,
                VERSION_LATEST_HASH,
                REPO_LINK,
                hash,
            )),
            Self::VoiceChannelCreated { channel_name } => {
                f.write_str(&format!("{} {}", VOICE_CHANNEL_CREATED, channel_name))
            },
            Self::VoteTopggVoted => f.write_str(VOTE_TOPGG_VOTED),
            Self::VoteTopggNotVoted => f.write_str(VOTE_TOPGG_NOT_VOTED),
            Self::Volume { vol, old_vol } => {
                f.write_str(&format!("{}: {}\n{}: {}", VOLUME, vol, OLD_VOLUME, old_vol))
            },
            Self::WaybackSnapshot { url } => f.write_str(&format!("{} {}", WAYBACK_SNAPSHOT, url)),
            Self::WelcomeSettings(settings) => f.write_str(settings),
            Self::GpSubmitted {
                title,
                replaced,
                submitted,
                of,
            } => {
                let lead = if *replaced {
                    GP_SUBMITTED_REPLACED
                } else {
                    GP_SUBMITTED
                };
                if *of > 0 {
                    f.write_str(&format!(
                        "{} **{}** ({} {} {} {})",
                        lead, title, submitted, GP_SUBMITTED_OF, of, "in the voice channel"
                    ))
                } else {
                    f.write_str(&format!("{} **{}** ({} in)", lead, title, submitted))
                }
            },
            Self::GpStarted {
                category,
                rounds,
                timer_secs,
                cleared_queue,
            } => f.write_str(&format!(
                "{} {} {} {} — {} {}{}",
                GP_STARTED,
                rounds,
                GP_STARTED_ROUNDS,
                category,
                crate::utils::duration_to_string(std::time::Duration::from_secs(*timer_secs)),
                GP_STARTED_TIMER,
                if *cleared_queue {
                    format!(" {}", GP_QUEUE_CLEARED)
                } else {
                    String::new()
                }
            )),
            Self::GpRoundSkipped => f.write_str(GP_ROUND_SKIPPED),
            Self::GpEnded { by } => f.write_str(&format!("{} {}", GP_ENDED_BY, by)),
            Self::GpWindowClosed { count } => f.write_str(&format!(
                "{} {} {}",
                GP_CLOSED_BY_HOST, count, GP_WINDOW_CLOSED_SONGS
            )),
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
    fn test_gp_messages_display() {
        use crate::messaging::messages::{
            GP_CLOSED_BY_HOST, GP_ENDED_BY, GP_QUEUE_CLEARED, GP_ROUND_SKIPPED, GP_STARTED,
            GP_SUBMITTED, GP_SUBMITTED_REPLACED, GP_WINDOW_CLOSED_SONGS,
        };

        let msg = CrackedMessage::GpSubmitted {
            title: "Never Gonna Give You Up".to_string(),
            replaced: false,
            submitted: 2,
            of: 4,
        };
        let s = msg.to_string();
        assert!(s.starts_with(GP_SUBMITTED), "{s}");
        assert!(s.contains("**Never Gonna Give You Up**"), "{s}");
        assert!(s.contains("2 in 4"), "{s}");
        let msg = CrackedMessage::GpSubmitted {
            title: "x".to_string(),
            replaced: true,
            submitted: 1,
            of: 0,
        };
        let s = msg.to_string();
        assert!(s.starts_with(GP_SUBMITTED_REPLACED), "{s}");
        assert!(s.ends_with("(1 in)"), "{s}");

        let msg = CrackedMessage::GpStarted {
            category: "🥹 Nostalgia",
            rounds: 5,
            timer_secs: 180,
            cleared_queue: false,
        };
        let s = msg.to_string();
        assert!(s.starts_with(&format!("{} 5", GP_STARTED)), "{s}");
        assert!(s.contains("🥹 Nostalgia"), "{s}");
        assert!(!s.contains(GP_QUEUE_CLEARED), "{s}");
        let msg = CrackedMessage::GpStarted {
            category: "🎲 Mixed",
            rounds: 3,
            timer_secs: 60,
            cleared_queue: true,
        };
        assert!(msg.to_string().ends_with(&format!(" {}", GP_QUEUE_CLEARED)));

        assert_eq!(
            CrackedMessage::GpWindowClosed { count: 3 }.to_string(),
            format!("{} 3 {}", GP_CLOSED_BY_HOST, GP_WINDOW_CLOSED_SONGS)
        );

        assert_eq!(CrackedMessage::GpRoundSkipped.to_string(), GP_ROUND_SKIPPED);
        assert_eq!(
            CrackedMessage::GpEnded {
                by: "alice".to_string()
            }
            .to_string(),
            format!("{} alice", GP_ENDED_BY)
        );

        // Appended after every existing variant, so the discriminants the
        // `test_discriminant` test pins are untouched.
        assert!(
            CrackedMessage::GpRoundSkipped.discriminant()
                > CrackedMessage::WelcomeSettings(String::new()).discriminant()
        );
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
            id: serenity::GenericChannelId::default(),
            name: "test".to_string(),
            size: 1,
        };
        assert_eq!(
            message,
            CrackedMessage::ChannelSizeSet {
                id: serenity::GenericChannelId::default(),
                name: "test".to_string(),
                size: 1
            }
        );

        let message = CrackedMessage::ChannelDeleted {
            channel_id: serenity::GenericChannelId::default(),
            channel_name: "test".to_string(),
        };
        assert_eq!(
            message,
            CrackedMessage::ChannelDeleted {
                channel_id: serenity::GenericChannelId::default(),
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
                id: serenity::GenericChannelId::default(),
                name: "test".to_string(),
                size: 1,
            }
        );

        let message = CrackedMessage::ChannelSizeSet {
            id: serenity::GenericChannelId::default(),
            name: "test".to_string(),
            size: 1,
        };
        assert_ne!(message, CrackedMessage::Clean(1));

        let message = CrackedMessage::ChannelDeleted {
            channel_id: serenity::GenericChannelId::default(),
            channel_name: "test".to_string(),
        };
        assert_ne!(
            message,
            CrackedMessage::ChannelSizeSet {
                id: serenity::GenericChannelId::default(),
                name: "test".to_string(),
                size: 1,
            }
        );
    }
}
