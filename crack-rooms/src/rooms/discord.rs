//! The only door voice rooms use for channel, member and permission calls, so
//! tests can record exactly what would be sent. Messages go through the
//! courier, never through here.
use super::mute_hold::VoiceNow;
use super::state::{OverwriteTarget, StoredOverwrite};
use serenity::all::{
    ChannelId, ChannelType, CreateChannel, EditChannel, EditMember, GuildId, Http, MessageId,
    PermissionOverwriteType, UserId,
};
use serenity::async_trait;
use serenity::http::{HttpError, JsonErrorCode};
use serenity::nonmax::NonMaxU16;
use std::sync::Arc;

/// The audit-log reason on every call rooms make.
const REASON: &str = "CrackTunes voice room";

/// What the mod log's audit reader skips: the room's server mute and its undo
/// are recorded by the room's own `RoomMute` line.
pub const ROOM_AUDIT_REASON_PREFIX: &str = "CrackTunes room";
/// The audit-log reason on a room's server mute and its undo.
pub const ROOM_MUTE_AUDIT_REASON: &str = "CrackTunes room: mute";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordError {
    /// Discord code 50013: the bot lacks a permission the call needs.
    MissingPermissions,
    /// Discord code 10003: the channel is gone.
    UnknownChannel,
    /// Discord code 40032: the member to move or disconnect is not in voice.
    NotConnected,
    Other(String),
}

impl From<serenity::Error> for DiscordError {
    fn from(err: serenity::Error) -> Self {
        if let serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) = &err {
            match response.error.code {
                JsonErrorCode::LackPermissionsForAction => return Self::MissingPermissions,
                JsonErrorCode::UnknownChannel => return Self::UnknownChannel,
                JsonErrorCode::TargetUserNotConnectedToVoice => return Self::NotConnected,
                _ => {},
            }
        }
        Self::Other(err.to_string())
    }
}

/// A room to create. No overwrites are sent: the channel inherits its
/// category's, and the bot adds its own afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoom {
    pub guild: GuildId,
    pub name: String,
    pub parent: Option<ChannelId>,
    pub user_limit: u8,
}

/// The channel Discord made, and the overwrites it came with (inherited from
/// the category).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedRoom {
    pub channel: ChannelId,
    pub overwrites: Vec<StoredOverwrite>,
}

#[async_trait]
pub trait RoomsDiscord: Send + Sync {
    async fn create_voice_channel(&self, r: &NewRoom) -> Result<CreatedRoom, DiscordError>;
    async fn delete_channel(&self, ch: ChannelId) -> Result<(), DiscordError>;
    async fn put_overwrite(&self, ch: ChannelId, o: &StoredOverwrite) -> Result<(), DiscordError>;
    async fn delete_overwrite(&self, ch: ChannelId, t: OverwriteTarget)
        -> Result<(), DiscordError>;
    async fn set_user_limit(&self, ch: ChannelId, limit: u8) -> Result<(), DiscordError>;
    async fn rename(&self, ch: ChannelId, name: &str) -> Result<(), DiscordError>;
    /// `None` disconnects the member from voice.
    async fn move_member(
        &self,
        guild: GuildId,
        user: UserId,
        to: Option<ChannelId>,
    ) -> Result<(), DiscordError>;
    async fn pin(&self, ch: ChannelId, msg: MessageId) -> Result<(), DiscordError>;
    /// Server-mute or unmute the member. Refused with `NotConnected` when
    /// they are not in voice.
    async fn set_mute(&self, guild: GuildId, user: UserId, on: bool) -> Result<(), DiscordError>;
    /// The member's voice state as Discord has it right now. A member who is
    /// not in voice is `VoiceNow::default()`.
    async fn voice_now(&self, guild: GuildId, user: UserId) -> Result<VoiceNow, DiscordError>;
}

/// The real Discord behind [`RoomsDiscord`]. Untested here; the TuneTitan
/// checklist covers it.
pub struct LiveRoomsDiscord {
    pub http: Arc<Http>,
}

#[async_trait]
impl RoomsDiscord for LiveRoomsDiscord {
    async fn create_voice_channel(&self, r: &NewRoom) -> Result<CreatedRoom, DiscordError> {
        let mut builder = CreateChannel::new(r.name.as_str())
            .kind(ChannelType::Voice)
            .audit_log_reason(REASON);
        if let Some(limit) = NonMaxU16::new(u16::from(r.user_limit)) {
            builder = builder.user_limit(limit);
        }
        if let Some(parent) = r.parent {
            builder = builder.category(parent);
        }
        let channel = r.guild.create_channel(&self.http, builder).await?;
        Ok(CreatedRoom {
            channel: channel.id,
            overwrites: channel
                .permission_overwrites
                .iter()
                .map(StoredOverwrite::from)
                .collect(),
        })
    }

    async fn delete_channel(&self, ch: ChannelId) -> Result<(), DiscordError> {
        ch.widen().delete(&self.http, Some(REASON)).await?;
        Ok(())
    }

    async fn put_overwrite(&self, ch: ChannelId, o: &StoredOverwrite) -> Result<(), DiscordError> {
        Ok(ch
            .create_permission(&self.http, o.to_serenity(), Some(REASON))
            .await?)
    }

    async fn delete_overwrite(
        &self,
        ch: ChannelId,
        t: OverwriteTarget,
    ) -> Result<(), DiscordError> {
        let kind = match t {
            OverwriteTarget::Member { id } => PermissionOverwriteType::Member(id),
            OverwriteTarget::Role { id } => PermissionOverwriteType::Role(id),
        };
        Ok(ch.delete_permission(&self.http, kind, Some(REASON)).await?)
    }

    async fn set_user_limit(&self, ch: ChannelId, limit: u8) -> Result<(), DiscordError> {
        let Some(limit) = NonMaxU16::new(u16::from(limit)) else {
            return Err(DiscordError::Other("user limit out of range".into()));
        };
        ch.edit(
            &self.http,
            EditChannel::new()
                .user_limit(limit)
                .audit_log_reason(REASON),
        )
        .await?;
        Ok(())
    }

    async fn rename(&self, ch: ChannelId, name: &str) -> Result<(), DiscordError> {
        ch.edit(
            &self.http,
            EditChannel::new().name(name).audit_log_reason(REASON),
        )
        .await?;
        Ok(())
    }

    async fn move_member(
        &self,
        guild: GuildId,
        user: UserId,
        to: Option<ChannelId>,
    ) -> Result<(), DiscordError> {
        let builder = match to {
            Some(ch) => EditMember::new().voice_channel(ch),
            None => EditMember::new().disconnect_member(),
        }
        .audit_log_reason(REASON);
        guild.edit_member(&self.http, user, builder).await?;
        Ok(())
    }

    async fn pin(&self, ch: ChannelId, msg: MessageId) -> Result<(), DiscordError> {
        Ok(ch.widen().pin(&self.http, msg, Some(REASON)).await?)
    }

    async fn set_mute(&self, guild: GuildId, user: UserId, on: bool) -> Result<(), DiscordError> {
        let builder = EditMember::new()
            .mute(on)
            .audit_log_reason(ROOM_MUTE_AUDIT_REASON);
        guild.edit_member(&self.http, user, builder).await?;
        Ok(())
    }

    async fn voice_now(&self, guild: GuildId, user: UserId) -> Result<VoiceNow, DiscordError> {
        match self.http.get_user_voice_state(guild, user).await {
            Ok(vs) => Ok(VoiceNow {
                channel: vs.channel_id,
                server_muted: vs.mute(),
            }),
            // Discord's answer for a member who is not in voice.
            Err(serenity::Error::Http(HttpError::UnsuccessfulRequest(response)))
                if response.error.code == JsonErrorCode::UnknownVoiceState =>
            {
                Ok(VoiceNow::default())
            },
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::http::ErrorResponse;

    async fn http_error(code: u32) -> serenity::Error {
        let body = format!(r#"{{"code": {code}, "message": "nope"}}"#);
        let response = reqwest13::Response::from(
            http::Response::builder()
                .status(403)
                .body(body)
                .expect("a valid response"),
        );
        serenity::Error::Http(HttpError::UnsuccessfulRequest(
            ErrorResponse::from_response(response, http::Method::PUT).await,
        ))
    }

    #[tokio::test]
    async fn discord_error_maps_50013_10003_and_40032() {
        assert_eq!(
            DiscordError::from(http_error(50013).await),
            DiscordError::MissingPermissions
        );
        assert_eq!(
            DiscordError::from(http_error(10003).await),
            DiscordError::UnknownChannel
        );
        assert_eq!(
            DiscordError::from(http_error(40032).await),
            DiscordError::NotConnected
        );
        // Anything else keeps its text.
        match DiscordError::from(http_error(10008).await) {
            DiscordError::Other(text) => assert!(!text.is_empty()),
            other => panic!("expected Other, got {other:?}"),
        }
    }
}
