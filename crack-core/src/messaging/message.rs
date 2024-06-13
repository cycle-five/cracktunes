use std::fmt::Display;

use self::serenity::model::mention::Mention;
use ::serenity::builder::CreateEmbed;
#[cfg(feature = "crack-osint")]
use crack_osint::virustotal::VirusTotalApiResponse;
use poise::serenity_prelude::{self as serenity, UserId};

use crate::{errors::CrackedError, messaging::messages::*};

const RELEASES_LINK: &str = "https://github.com/cycle-five/cracktunes/releases";
const REPO_LINK: &str = "https://github.com/cycle-five/cracktunes/";

#[repr(u8)]
#[derive(Debug)]
pub enum CrackedMessage {
    AutopauseOff,
    AutopauseOn,
    AutoplayOff,
    AutoplayOn,
    BugNone(String),
    CategoryCreated {
        channel_id: serenity::ChannelId,
        channel_name: String,
    },
    CountryName(String),
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
    DomainInfo(String),
    Error,
    ErrorHttp(serenity::http::HttpError),
    InvalidIP(String),
    IPDetails(String),
    IPVersion(String),
    Leaving,
    LoopDisable,
    LoopEnable,
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
    Premium(bool),
    PremiumPlug,
    RemoveMultiple,
    Resume,
    RoleCreated {
        role_id: serenity::RoleId,
        role_name: String,
    },
    RoleDeleted {
        role_id: serenity::RoleId,
        role_name: String,
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
    Shuffle,
    Skip,
    SkipAll,
    SkipTo {
        title: String,
        url: String,
    },
    Stop,
    SubcommandNotFound {
        group: String,
        subcommand: String,
    },
    SocialMediaResponse {
        response: String,
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
        channel_name: String,
    },
    UserAuthorized {
        id: UserId,
        mention: Mention,
        guild_id: serenity::GuildId,
        guild_name: String,
    },
    UserDeauthorized {
        id: UserId,
        mention: Mention,
        guild_id: serenity::GuildId,
        guild_name: String,
    },
    UserTimeout {
        id: UserId,
        mention: Mention,
        timeout_until: String,
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
    WaybackSnapshot {
        url: String,
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
            Self::BugNone(variable) => f.write_str(&format!("{} {} {}", BUG, variable, BUG_END)),
            Self::InvalidIP(ip) => f.write_str(&format!("{} {}", ip, FAIL_INVALID_IP)),
            Self::IPDetails(ip) => f.write_str(&format!("{} **{}**", IP_DETAILS, ip)),
            Self::IPVersion(ipv) => f.write_str(&format!("**{}**", ipv)),
            Self::AutopauseOff => f.write_str(AUTOPAUSE_OFF),
            Self::AutopauseOn => f.write_str(AUTOPAUSE_ON),
            Self::CountryName(name) => f.write_str(name),
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
            Self::DomainInfo(info) => f.write_str(info),
            Self::Error => f.write_str(ERROR),
            Self::ErrorHttp(err) => f.write_str(&format!("{}", err)),
            Self::Leaving => f.write_str(LEAVING),
            Self::LoopDisable => f.write_str(LOOP_DISABLED),
            Self::LoopEnable => f.write_str(LOOP_ENABLED),
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
            Self::SubcommandNotFound { group, subcommand } => f.write_str(&format!(
                "{}",
                SUBCOMMAND_NOT_FOUND
                    .replace("{group}", group)
                    .replace("{subcommand}", subcommand)
            )),
            Self::VoteSkip { mention, missing } => f.write_str(&format!(
                "{}{} {} {} {}",
                SKIP_VOTE_EMOJI, mention, SKIP_VOTE_USER, missing, SKIP_VOTE_MISSING
            )),
            Self::SocialMediaResponse { response } => f.write_str(response),
            Self::SongQueued { title, url } => {
                f.write_str(&format!("{} [**{}**]({})", ADDED_QUEUE, title, url))
            },
            Self::Seek { timestamp } => f.write_str(&format!("{} **{}**!", SEEKED, timestamp)),
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
                "{} [{}]({}/tag/v{})\n{}({}/latest)\n{}({}/tree/{})",
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
            Self::WaybackSnapshot { url } => f.write_str(&format!("{} {}", WAYBACK_SNAPSHOT, url)),
        }
    }
}

impl From<CrackedMessage> for String {
    fn from(message: CrackedMessage) -> Self {
        message.to_string()
    }
}

impl From<CrackedMessage> for CreateEmbed {
    fn from(message: CrackedMessage) -> Self {
        CreateEmbed::default().description(message.to_string())
    }
}

impl From<CrackedError> for CrackedMessage {
    fn from(error: CrackedError) -> Self {
        Self::CrackedError(error)
    }
}

impl From<CrackedMessage> for Result<CrackedMessage, CrackedError> {
    fn from(message: CrackedMessage) -> Self {
        Ok(message)
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
        assert_eq!(message.discriminant(), 9);
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
