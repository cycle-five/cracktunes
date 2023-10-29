use std::fmt::Display;

use self::serenity::model::mention::Mention;
use poise::serenity_prelude::{self as serenity, UserId};

use crate::messaging::messages::*;

const RELEASES_LINK: &str = "https://github.com/cycle-five/cracktunes/releases";

#[derive(Debug)]
pub enum CrackedMessage {
    AutopauseOff,
    AutopauseOn,
    CountryName(String),
    ChannelDeleted {
        channel_id: serenity::ChannelId,
        channel_name: String,
    },
    Clear,
    DomainInfo(String),
    Error,
    InvalidIP(String),
    IPDetails(String),
    IPVersion(String),
    Leaving,
    LoopDisable,
    LoopEnable,
    NowPlaying,
    Other(String),
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
    PlaylistCreated(String),
    PlaylistQueued,
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
    ScanResult {
        result: String,
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
    Summon {
        mention: Mention,
    },
    TextChannelCreated {
        channel_id: serenity::ChannelId,
        channel_name: String,
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
    UserUndeafened {
        user: String,
        user_id: UserId,
    },
    WaybackSnapshot {
        url: String,
    },
    Version {
        current: String,
    },
    VoteSkip {
        mention: Mention,
        missing: usize,
    },
    VoiceChannelCreated {
        channel_name: String,
    },
}

impl Display for CrackedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIP(ip) => f.write_str(&format!("{} {}", ip, INVALID_IP)),
            Self::IPDetails(ip) => f.write_str(&format!("{} **{}**", IP_DETAILS, ip)),
            Self::IPVersion(ipv) => f.write_str(&format!("**{}**", ipv)),
            Self::AutopauseOff => f.write_str(AUTOPAUSE_OFF),
            Self::AutopauseOn => f.write_str(AUTOPAUSE_ON),
            Self::CountryName(name) => f.write_str(name),
            Self::Clear => f.write_str(CLEARED),
            Self::ChannelDeleted {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{} {} {}",
                CHANNEL_DELETED, channel_id, channel_name
            )),
            Self::DomainInfo(info) => f.write_str(info),
            Self::Error => f.write_str(ERROR),
            Self::Leaving => f.write_str(LEAVING),
            Self::LoopDisable => f.write_str(LOOP_DISABLED),
            Self::LoopEnable => f.write_str(LOOP_ENABLED),
            Self::NowPlaying => f.write_str(QUEUE_NOW_PLAYING),
            Self::Other(message) => f.write_str(message),
            Self::PasswordPwned => f.write_str(PASSWORD_PWNED),
            Self::PasswordSafe => f.write_str(PASSWORD_SAFE),
            Self::Pause => f.write_str(PAUSED),
            Self::Paywall(url) => f.write_str(&format!("{}{}", ONETWOFT, url)),
            Self::PhoneNumberInfo(info) => f.write_str(info),
            Self::PhoneNumberInfoError => f.write_str(PHONE_NUMBER_INFO_ERROR),
            Self::PlaylistCreated(name) => {
                f.write_str(&format!("{} **{}**", PLAYLIST_CREATED, name))
            }
            Self::PlaylistQueued => f.write_str(PLAY_PLAYLIST),
            Self::PlayAllFailed => f.write_str(PLAY_ALL_FAILED),
            Self::PlayDomainBanned { domain } => {
                f.write_str(&format!("⚠️ **{}** {}", domain, PLAY_FAILED_BLOCKED_DOMAIN))
            }
            Self::ScanResult { result } => f.write_str(result),
            Self::Search => f.write_str(SEARCHING),
            Self::RemoveMultiple => f.write_str(REMOVED_QUEUE_MULTIPLE),
            Self::Resume => f.write_str(RESUMED),
            Self::RoleCreated { role_id, role_name } => {
                f.write_str(&format!("{} {} {}", ROLE_CREATED, role_id, role_name))
            }
            Self::RoleDeleted { role_id, role_name } => {
                f.write_str(&format!("{} {} {}", ROLE_DELETED, role_id, role_name))
            }
            Self::Shuffle => f.write_str(SHUFFLED_SUCCESS),
            Self::Stop => f.write_str(STOPPED),
            Self::VoteSkip { mention, missing } => f.write_str(&format!(
                "{}{} {} {} {}",
                SKIP_VOTE_EMOJI, mention, SKIP_VOTE_USER, missing, SKIP_VOTE_MISSING
            )),
            Self::SocialMediaResponse { response } => f.write_str(response),
            Self::Seek { timestamp } => f.write_str(&format!("{} **{}**!", SEEKED, timestamp)),
            Self::Skip => f.write_str(SKIPPED),
            Self::SkipAll => f.write_str(SKIPPED_ALL),
            Self::SkipTo { title, url } => {
                f.write_str(&format!("{} [**{}**]({})!", SKIPPED_TO, title, url))
            }
            Self::Summon { mention } => f.write_str(&format!("{} **{}**!", JOINING, mention)),
            Self::TextChannelCreated {
                channel_id,
                channel_name,
            } => f.write_str(&format!(
                "{} {} {}",
                TEXT_CHANNEL_CREATED, channel_id, channel_name
            )),
            Self::WaybackSnapshot { url } => f.write_str(&format!("{} {}", WAYBACK_SNAPSHOT, url)),
            Self::UserKicked { user_id } => f.write_str(&format!("{} {}", KICKED, user_id)),
            Self::UserBanned { user, user_id } => {
                f.write_str(&format!("{} {} {}", BANNED, user, user_id))
            }
            Self::UserUnbanned { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNBANNED, user, user_id))
            }
            Self::UserUndeafened { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNDEAFENED, user, user_id))
            }
            Self::UserDeafened { user, user_id } => {
                f.write_str(&format!("{} {} {}", DEAFENED, user, user_id))
            }
            Self::UserMuted { user, user_id } => {
                f.write_str(&format!("{} {} {}", MUTED, user, user_id))
            }
            Self::UserUnmuted { user, user_id } => {
                f.write_str(&format!("{} {} {}", UNMUTED, user, user_id))
            }
            Self::Version { current } => f.write_str(&format!(
                "{} [{}]({}/tag/v{})\n{}({}/latest)",
                VERSION, current, RELEASES_LINK, current, VERSION_LATEST, RELEASES_LINK
            )),
            Self::VoiceChannelCreated { channel_name } => {
                f.write_str(&format!("{} {}", VOICE_CHANNEL_CREATED, channel_name))
            }
        }
    }
}

impl From<CrackedMessage> for String {
    fn from(message: CrackedMessage) -> Self {
        message.to_string()
    }
}
