use std::fmt::Display;

use self::serenity::model::mention::Mention;
use ::serenity::builder::CreateEmbed;
#[cfg(feature = "crack-osint")]
use crack_osint::virustotal::VirusTotalApiResponse;
use poise::serenity_prelude::{self as serenity, UserId};

use crate::{errors::CrackedError, messaging::messages::*};

const RELEASES_LINK: &str = "https://github.com/cycle-five/cracktunes/releases";
const REPO_LINK: &str = "https://github.com/cycle-five/cracktunes/";

#[derive(Debug)]
pub enum CrackedMessage {
    AutopauseOff,
    AutopauseOn,
    AutoplayOff,
    AutoplayOn,
    CategoryCreated {
        channel_id: serenity::ChannelId,
        channel_name: String,
    },
    CountryName(String),
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
        user_id: UserId,
        user_name: String,
        guild_id: serenity::GuildId,
        guild_name: String,
    },
    UserDeauthorized {
        user_id: UserId,
        user_name: String,
        guild_id: serenity::GuildId,
        guild_name: String,
    },
    UserTimeout {
        user: String,
        user_id: String,
        timeout_until: String,
    },
    UserKicked {
        user_id: UserId,
    },
    UserBanned {
        user: String,
        user_id: UserId,
    },
    UserUnbanned {
        user: String,
        user_id: UserId,
    },
    UserMuted {
        user: String,
        user_id: UserId,
    },
    UserUnmuted {
        user: String,
        user_id: UserId,
    },
    UserDeafened {
        user: String,
        user_id: UserId,
    },
    UserDeafenedFail {
        user: String,
        user_id: UserId,
    },
    UserUndeafened {
        user: String,
        user_id: UserId,
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

impl Display for CrackedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AutoplayOff => f.write_str(AUTOPLAY_OFF),
            Self::AutoplayOn => f.write_str(AUTOPLAY_ON),
            Self::InvalidIP(ip) => f.write_str(&format!("{} {}", ip, FAIL_INVALID_IP)),
            Self::IPDetails(ip) => f.write_str(&format!("{} **{}**", IP_DETAILS, ip)),
            Self::IPVersion(ipv) => f.write_str(&format!("**{}**", ipv)),
            Self::AutopauseOff => f.write_str(AUTOPAUSE_OFF),
            Self::AutopauseOn => f.write_str(AUTOPAUSE_ON),
            Self::CountryName(name) => f.write_str(name),
            Self::Clear => f.write_str(CLEARED),
            Self::Clean(n) => f.write_str(&format!("{} {}!", CLEANED, n)),
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
                user_id,
                user_name,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{}\n User: {} ({}) Guild: {} ({})",
                AUTHORIZED, user_name, user_id, guild_name, guild_id
            )),
            Self::UserDeauthorized {
                user_id,
                user_name,
                guild_id,
                guild_name,
            } => f.write_str(&format!(
                "{}\n User: {} ({}) Guild: {} ({})",
                DEAUTHORIZED, user_name, user_id, guild_name, guild_id
            )),
            Self::UserTimeout {
                user: _,
                user_id,
                timeout_until,
            } => f.write_str(&format!(
                "User timed out: {} for {}",
                user_id, timeout_until
            )),
            Self::UserKicked { user_id } => f.write_str(&format!("{} {}", KICKED, user_id)),
            Self::UserBanned { user, user_id } => {
                f.write_str(&format!("{} {} {}", BANNED, user, user_id))
            },
            Self::UserUnbanned { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNBANNED, user, user_id))
            },
            Self::UserUndeafened { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNDEAFENED, user, user_id))
            },
            Self::UserDeafened { user, user_id } => {
                f.write_str(&format!("{}\n{}({})", DEAFENED, user.mention(), user_id))
            },
            Self::UserDeafenedFail { user, user_id } => f.write_str(&format!(
                "{}\n{}({})",
                DEAFENED_FAIL,
                user.mention(),
                user_id
            )),
            Self::UserMuted { user, user_id } => {
                f.write_str(&format!("{} {} {}", MUTED, user, user_id))
            },
            Self::UserUnmuted { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNMUTED, user, user_id))
            },
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
