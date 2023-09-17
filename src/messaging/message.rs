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
    PlayDomainBanned { domain: String },
    PlaylistCreated(String),
    PlaylistQueued,
    RemoveMultiple,
    Resume,
    ScanResult { result: String },
    Search,
    Seek { timestamp: String },
    Shuffle,
    Skip,
    SkipAll,
    SkipTo { title: String, url: String },
    Stop,
    SocialMediaResponse { response: String },
    Summon { mention: Mention },
    UserKicked { user_id: UserId },
    UserBanned { user: String, user_id: UserId },
    UserMuted { user: String, user_id: UserId },
    UserDeafened { user: String, user_id: UserId },
    WaybackSnapshot { url: String },
    Version { current: String },
    VoteSkip { mention: Mention, missing: usize },
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
            Self::WaybackSnapshot { url } => f.write_str(&format!("{} {}", WAYBACK_SNAPSHOT, url)),
            Self::UserKicked { user_id } => f.write_str(&format!("{} {}", KICKED, user_id)),
            Self::UserBanned { user, user_id } => {
                f.write_str(&format!("{} {} {}", BANNED, user, user_id))
            }
            Self::UserDeafened { user, user_id } => {
                f.write_str(&format!("{} {} {}", DEAFENED, user, user_id))
            }
            Self::UserMuted { user, user_id } => {
                f.write_str(&format!("{} {} {}", MUTED, user, user_id))
            }
            Self::Version { current } => f.write_str(&format!(
                "{} [{}]({}/tag/v{})\n{}({}/latest)",
                VERSION, current, RELEASES_LINK, current, VERSION_LATEST, RELEASES_LINK
            )),
        }
    }
}
