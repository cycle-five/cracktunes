use crate::commands::play_utils::TrackReadyData;
use crate::commands::{has_voted_bot_id, MyAuxMetadata};
use crate::db;
use crate::db::{MetadataMsg, PlayLog};
use crate::guild::operations::GuildSettingsOperations;
use crate::guild::settings::GuildSettings;
use crate::http_utils;
use crate::Error;
use crate::{
    commands::CrackedError, http_utils::SendMessageParams, messaging::message::CrackedMessage,
    utils::OptionTryUnwrap, CrackedResult, Data,
};
use colored::Colorize;
use poise::serenity_prelude as serenity;
use poise::{CreateReply, ReplyHandle};
use serenity::all::{ChannelId, CreateEmbed, GuildId, Message, UserId};
use songbird::tracks::{PlayMode, TrackQueue};
use songbird::Call;
use std::{future::Future, sync::Arc};
use tokio::sync::Mutex;

use crate::messaging::interface;
/// TODO: Separate all the messaging related functions from the other extensions and
/// put them into this extension.
pub trait MessageInterfaceCtxExt {
    /// Send a message notifying the user they found a command.
    fn send_found_command(
        &self,
        command: String,
    ) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Send a message to the user with the invite link for the bot.
    fn send_invite_link(&self) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Creates a reply from a CrackedMessage and sends its, optionally as an embed.
    fn send_reply(
        &self,
        message: CrackedMessage,
        as_embed: bool,
    ) -> impl Future<Output = Result<ReplyHandle<'_>, CrackedError>>;

    /// Creates an embed from a CrackedMessage and sends it.
    fn send_reply_embed(
        &self,
        message: CrackedMessage,
    ) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Sends a message ecknowledging that the user has grabbed the current track.
    fn send_grabbed_notice(&self) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Send a now playing message
    fn send_now_playing(&self, chan_id: ChannelId) -> impl Future<Output = Result<Message, Error>>;

    /// Return whether the queue is paused or not.
    fn is_paused(&self) -> impl Future<Output = Result<bool, CrackedError>>;
}

impl MessageInterfaceCtxExt for crate::Context<'_> {
    /// Sends a message notifying the use they found a command.
    async fn send_found_command(&self, command: String) -> Result<ReplyHandle, Error> {
        self.send_reply_embed(CrackedMessage::CommandFound(command))
            .await
    }

    async fn send_invite_link(&self) -> Result<ReplyHandle, Error> {
        self.send_reply_embed(CrackedMessage::InviteLink).await
    }

    async fn send_reply(
        &self,
        message: CrackedMessage,
        as_embed: bool,
    ) -> Result<ReplyHandle, CrackedError> {
        PoiseContextExt::send_reply(self, message, as_embed).await
    }

    async fn send_reply_embed(&self, message: CrackedMessage) -> Result<ReplyHandle, Error> {
        PoiseContextExt::send_reply(self, message, true)
            .await
            .map_err(Into::into)
    }

    async fn send_grabbed_notice(&self) -> Result<ReplyHandle, Error> {
        self.send_reply_embed(CrackedMessage::GrabbedNotice).await
    }

    async fn send_now_playing(&self, chan_id: ChannelId) -> Result<Message, Error> {
        let call = self.get_call().await?;
        // We don't add this message to the cache because we shouldn't delete it.
        interface::send_now_playing(
            chan_id,
            self.serenity_context().http.clone(),
            call,
            //cur_pos,
            //metadata,
        )
        .await
    }

    async fn is_paused(&self) -> CrackedResult<bool> {
        let call = self.get_call().await?;
        let handler = call.lock().await;
        let topt = handler.queue().current();
        if let Some(t) = topt {
            Ok(t.get_info().await?.playing == PlayMode::Pause)
        } else {
            Ok(false)
        }
    }
}

/// Trait to extend the Context struct with additional convenience functionality.
pub trait ContextExt {
    /// Send a message to tell the worker pool to do a db write when it feels like it.
    fn send_track_metadata_write_msg(&self, ready_track: &TrackReadyData);
    fn async_send_track_metadata_write_msg(
        &self,
        ready_track: &TrackReadyData,
    ) -> impl Future<Output = CrackedResult<()>>;
    /// The the user id for the author of the message that created this context.
    fn get_user_id(&self) -> serenity::UserId;
    /// Gets the log of last played songs on the bot by a specific user
    fn get_last_played_by_user(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<String>, CrackedError>>;

    fn get_guild_settings(&self, guild_id: GuildId) -> impl Future<Output = Option<GuildSettings>>;

    /// Gets the log of last played songs on the bot
    fn get_last_played(&self) -> impl Future<Output = Result<Vec<String>, CrackedError>>;
    /// Return the call that the bot is currently in, if it is in one.
    fn get_call(&self) -> impl Future<Output = Result<Arc<Mutex<Call>>, CrackedError>>;
    /// Return the call and the guild id. This is convenience function I found I had many cases for.
    fn get_call_guild_id(
        &self,
    ) -> impl Future<Output = Result<(Arc<Mutex<Call>>, GuildId), CrackedError>>;
    /// Return the queue owned.
    fn get_queue(&self) -> impl Future<Output = Result<TrackQueue, CrackedError>>;
    /// Return the db pool for database operations.
    fn get_db_pool(&self) -> Result<sqlx::PgPool, CrackedError>;
    /// Add a message to the cache
    fn add_msg_to_cache(
        &self,
        guild_id: GuildId,
        msg: Message,
    ) -> impl Future<Output = Option<Message>>;
    /// Gets the channel id that the bot is currently playing in for a given guild.
    fn get_active_channel_id(&self, guild_id: GuildId) -> impl Future<Output = Option<ChannelId>>;

    // ----- Send message utility functions ------ //

    /// Send a message notifying the user they found a command.
    fn send_found_command(
        &self,
        command: String,
    ) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Send a message to the user with the invite link for the bot.
    fn send_invite_link(&self) -> impl Future<Output = Result<ReplyHandle<'_>, Error>>;

    /// Check if the authoring user has voted for the bot on several sites within the last 12 hours.
    fn check_and_record_vote(&self) -> impl Future<Output = Result<bool, CrackedError>>;
}

/// Implement the ContextExt trait for the Context struct.
impl ContextExt for crate::Context<'_> {
    /// Get the user id from a context.
    fn get_user_id(&self) -> serenity::UserId {
        match self {
            poise::Context::Application(ctx) => ctx.interaction.user.id,
            poise::Context::Prefix(ctx) => ctx.msg.author.id,
        }
    }

    /// Get the guild settings for a guild.
    async fn get_guild_settings(&self, guild_id: GuildId) -> Option<GuildSettings> {
        self.data().get_guild_settings(guild_id).await
    }

    /// Get the last played songs for a user.
    async fn get_last_played_by_user(&self, user_id: UserId) -> Result<Vec<String>, CrackedError> {
        let guild_id = self.guild_id().ok_or(CrackedError::NoGuildId)?;
        PlayLog::get_last_played(
            self.data().database_pool.as_ref().unwrap(),
            Some(user_id.get() as i64),
            Some(guild_id.get() as i64),
        )
        .await
        .map_err(|e| e.into())
    }

    /// Get the last played songs for a guild.
    async fn get_last_played(&self) -> Result<Vec<String>, CrackedError> {
        let guild_id = self.guild_id().ok_or(CrackedError::NoGuildId)?;
        PlayLog::get_last_played(
            self.data().database_pool.as_ref().unwrap(),
            None,
            Some(guild_id.get() as i64),
        )
        .await
        .map_err(|e| e.into())
    }

    async fn async_send_track_metadata_write_msg(
        &self,
        _ready_track: &TrackReadyData,
    ) -> CrackedResult<()> {
        todo!()
    }

    // /// Send a message to tell the worker pool to do a db write when it feels like it.
    // async fn async_send_track_metadata_write_msg(
    //     &self,
    //     ready_track: TrackReadyData,
    // ) -> CrackedResult<()> {
    //     let username = ready_track.username.clone();
    //     let MyAuxMetadata(aux_metadata) = ready_track.metadata.clone();
    //     let user_id = ready_track.user_id.clone();
    //     let guild_id = self.guild_id().unwrap();
    //     let channel_id = self.channel_id();

    //     let write_data: MetadataMsg = MetadataMsg {
    //         aux_metadata,
    //         user_id,
    //         username,
    //         guild_id,
    //         channel_id,
    //     };

    //     let pool = self.data().get_db_pool().unwrap();
    //     write_metadata_pg(&pool, write_data).await?;
    //     Ok(())
    // }

    /// Send a message to tell the worker pool to do a db write when it feels like it.
    fn send_track_metadata_write_msg(&self, ready_track: &TrackReadyData) {
        let username = ready_track.username.clone();
        let MyAuxMetadata(aux_metadata) = ready_track.metadata.clone();
        let user_id = ready_track.user_id;
        let guild_id = self.guild_id().unwrap();
        let channel_id = self.channel_id();
        if let Some(channel) = &self.data().db_channel {
            let write_data: MetadataMsg = MetadataMsg {
                aux_metadata,
                user_id,
                username,
                guild_id,
                channel_id,
            };
            if let Err(e) = channel.try_send(write_data) {
                tracing::error!("Error sending metadata to db_channel: {}", e);
            }
        }
    }

    /// Return the call that the bot is currently in, if it is in one.
    async fn get_call(&self) -> Result<Arc<Mutex<Call>>, CrackedError> {
        let guild_id = self.guild_id().ok_or(CrackedError::NoGuildId)?;
        let manager = songbird::get(self.serenity_context())
            .await
            .ok_or(CrackedError::NotConnected)?;
        manager.get(guild_id).ok_or(CrackedError::NotConnected)
    }

    /// Return the call that the bot is currently in, if it is in one.
    async fn get_call_guild_id(&self) -> Result<(Arc<Mutex<Call>>, GuildId), CrackedError> {
        let guild_id = self.guild_id().ok_or(CrackedError::NoGuildId)?;
        let manager = songbird::get(self.serenity_context())
            .await
            .ok_or(CrackedError::NotConnected)?;
        manager
            .get(guild_id)
            .map(|x| (x, guild_id))
            .ok_or(CrackedError::NotConnected)
    }

    /// Get the queue owned.
    async fn get_queue(&self) -> Result<TrackQueue, CrackedError> {
        let lock = self.get_call().await?;
        let call = lock.lock().await;
        Ok(call.queue().clone())
    }

    /// Get the database pool
    fn get_db_pool(&self) -> Result<sqlx::PgPool, CrackedError> {
        self.data().get_db_pool()
    }

    async fn add_msg_to_cache(&self, guild_id: GuildId, msg: Message) -> Option<Message> {
        self.data().add_msg_to_cache(guild_id, msg).await
    }

    /// Gets the channel id that the bot is currently playing in for a given guild.
    async fn get_active_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let serenity_context = self.serenity_context();
        let manager = songbird::get(serenity_context)
            .await
            .expect("Failed to get songbird manager")
            .clone();

        let call_lock = manager.get(guild_id)?;
        let call = call_lock.lock().await;

        let channel_id = call.current_channel()?;
        let serenity_channel_id = ChannelId::new(channel_id.0.into());

        Some(serenity_channel_id)
    }

    // ----- Send message utility functions ------ //

    /// Sends a message notifying the use they found a command.
    async fn send_found_command(&self, command: String) -> Result<ReplyHandle, Error> {
        self.send_reply_embed(CrackedMessage::CommandFound(command))
            .await
    }

    async fn send_invite_link(&self) -> Result<ReplyHandle, Error> {
        self.send_reply_embed(CrackedMessage::InviteLink).await
    }

    // ----------- DB Write functions ----------- //

    async fn check_and_record_vote(&self) -> Result<bool, CrackedError> {
        let user_id: UserId = self.author().id;
        let bot_id: UserId = http_utils::get_bot_id(self).await?;
        let pool = self.get_db_pool()?;
        let has_voted = has_voted_bot_id(
            http_utils::get_client().clone(),
            u64::from(bot_id),
            u64::from(user_id),
        )
        .await?;
        let has_voted_db =
            db::UserVote::has_voted_recently_topgg(i64::from(user_id), &pool).await?;
        let record_vote = has_voted && !has_voted_db;

        if record_vote {
            let username = self.author().name.clone();
            db::User::insert_or_update_user(&pool, i64::from(user_id), username).await?;
            db::UserVote::insert_user_vote(&pool, i64::from(user_id), "top.gg".to_string()).await?;
        }

        Ok(has_voted)
    }
}

/// Extension trait for the poise::Context.
pub trait PoiseContextExt<'ctx> {
    // async fn send_error(
    //     &'ctx self,
    //     error_message: impl Into<Cow<'ctx, str>>,
    // ) -> CrackedResult<Option<poise::ReplyHandle<'ctx>>>;
    // async fn send_ephemeral(
    //     &'ctx self,
    //     message: impl Into<Cow<'ctx, str>>,
    // ) -> CrackedResult<poise::ReplyHandle<'ctx>>;
    fn author_vc(&self) -> Option<serenity::ChannelId>;
    fn author_permissions(&self) -> impl Future<Output = CrackedResult<serenity::Permissions>>;
    fn is_prefix(&self) -> bool;
    fn send_reply(
        &self,
        message: CrackedMessage,
        as_embed: bool,
    ) -> impl Future<Output = Result<ReplyHandle<'ctx>, CrackedError>>;
    fn send_message(
        &self,
        params: SendMessageParams,
    ) -> impl Future<Output = Result<ReplyHandle<'ctx>, CrackedError>>;
    fn send_embed_response(
        &self,
        embed: CreateEmbed,
    ) -> impl Future<Output = CrackedResult<ReplyHandle<'ctx>>>;
}

/// Implementation of the extension trait for the poise::Context.
impl<'ctx> PoiseContextExt<'ctx> for crate::Context<'ctx> {
    /// Checks if we're in a prefix context or not.
    fn is_prefix(&self) -> bool {
        matches!(self, crate::Context::Prefix(_))
    }

    /// Get the VC that author of the incoming message is in if any.
    fn author_vc(&self) -> Option<serenity::ChannelId> {
        require_guild!(self, None)
            .voice_states
            .get(&self.author().id)
            .and_then(|vc| vc.channel_id)
    }

    /// Creates an embed from a CrackedMessage and sends it as an embed.
    async fn send_reply(
        &self,
        message: CrackedMessage,
        as_embed: bool,
    ) -> Result<ReplyHandle<'ctx>, CrackedError> {
        let color = serenity::Colour::from(&message);
        let embed: Option<CreateEmbed> = <Option<CreateEmbed>>::from(&message);
        let params = SendMessageParams::new(message)
            .with_color(color)
            .with_as_embed(as_embed)
            .with_embed(embed);
        let handle = self.send_message(params).await?;
        Ok(handle)
    }

    /// Base, very generic send message function.
    async fn send_message(
        &self,
        params: SendMessageParams,
    ) -> Result<ReplyHandle<'ctx>, CrackedError> {
        //let channel_id = send_params.channel;
        let as_embed = params.as_embed;
        let as_reply = params.reply;
        let as_ephemeral = params.ephemeral;
        let text = params.msg.to_string();
        let reply = if as_embed {
            let embed = params
                .embed
                .unwrap_or(CreateEmbed::default().description(text).color(params.color));
            CreateReply::default().embed(embed)
        } else {
            let c = colored::Color::TrueColor {
                r: params.color.r(),
                g: params.color.g(),
                b: params.color.b(),
            };
            CreateReply::default().content(text.color(c).to_string())
        };
        let reply = reply.reply(as_reply).ephemeral(as_ephemeral);
        let handle = self.send(reply).await?;
        if params.cache_msg {
            let msg = handle.clone().into_message().await?;
            self.data()
                .add_msg_to_cache(self.guild_id().unwrap(), msg)
                .await;
        }
        Ok(handle)
    }

    async fn send_embed_response(&self, embed: CreateEmbed) -> CrackedResult<ReplyHandle<'ctx>> {
        let is_ephemeral = false;
        let is_reply = true;
        let params = SendMessageParams::default()
            .with_ephemeral(is_ephemeral)
            .with_embed(Some(embed))
            .with_reply(is_reply);

        self.send_message(params).await
    }

    //     // async fn neutral_colour(&self) -> u32 {
    //     //     if let Some(guild_id) = self.guild_id() {
    //     //         let row = self.data().guilds_db.get(guild_id.get() as i64).await;
    //     //         if row
    //     //             .map(|row| row.voice_mode)
    //     //             .map_or(false, TTSMode::is_premium)
    //     //         {
    //     //             return PREMIUM_NEUTRAL_COLOUR;
    //     //         }
    //     //     }

    //     //     FREE_NEUTRAL_COLOUR
    //     // }

    /// Get the permissions of the calling user in the guild.
    async fn author_permissions(&self) -> CrackedResult<serenity::Permissions> {
        // Handle non-guild call first, to allow try_unwrap calls to be safe.
        if self.guild_id().is_none() {
            return Ok(((serenity::Permissions::from_bits_truncate(
                0b111_1100_1000_0000_0000_0111_1111_1000_0100_0000,
            ) | serenity::Permissions::SEND_MESSAGES)
                - serenity::Permissions::SEND_TTS_MESSAGES)
                - serenity::Permissions::MANAGE_MESSAGES);
        }

        // Accesses guild cache and is asynchronous, must be called first.
        let member = self.author_member().await.try_unwrap()?;

        // Accesses guild cache, but the member above was cloned out, so safe.
        let guild = self.guild().try_unwrap()?;

        // Does not access cache, but relies on above guild cache reference.
        let channel = guild.channels.get(&self.channel_id()).try_unwrap()?;

        // Does not access cache.
        Ok(guild.user_permissions_in(channel, &member))
    }
}
//     async fn send_ephemeral(
//         &'ctx self,
//         message: impl Into<Cow<'ctx, str>>,
//     ) -> CrackedResult<poise::ReplyHandle<'ctx>> {
//         let reply = poise::CreateReply::default().content(message);
//         let handle = self.send(reply).await?;
//         Ok(handle)
//     }

//     async fn send_reply_embed(
//         self,
//         message: CrackedMessage,
//     ) -> CrackedResult<poise::ReplyHandle<'ctx>> {
//         let handle = utils::send_reply_embed(&self, message).await?;
//         Ok(handle)
//     }

//     #[cold]
//     async fn send_error(
//         &'ctx self,
//         error_message: impl Into<Cow<'ctx, str>>,
//     ) -> CrackedResult<Option<poise::ReplyHandle<'ctx>>> {
//         let author = self.author();
//         let serenity_ctx = self.serenity_context();
//         let serernity_cache = &serenity_ctx.cache;

//         let (name, avatar_url) = match self.channel_id().to_channel(serenity_ctx).await? {
//             serenity::Channel::Guild(channel) => {
//                 let permissions = channel
//                     .permissions_for_user(serernity_cache, serernity_cache.current_user().id)?;

//                 if !permissions.send_messages() {
//                     return Ok(None);
//                 };

//                 if !permissions.embed_links() {
//                     return self.send(poise::CreateReply::default()
//                         .ephemeral(true)
//                         .content("An Error Occurred! Please give me embed links permissions so I can tell you more!")
//                     ).await.map(Some).map_err(Into::into);
//                 };

//                 match channel.guild_id.member(serenity_ctx, author.id).await {
//                     Ok(member) => {
//                         let face = member.face();
//                         let display_name = member
//                             .nick
//                             .or(member.user.global_name)
//                             .unwrap_or(member.user.name);

//                         (Cow::Owned(display_name.to_string()), face)
//                     },
//                     Err(_) => (Cow::Borrowed(&*author.name), author.face()),
//                 }
//             },
//             serenity::Channel::Private(_) => (Cow::Borrowed(&*author.name), author.face()),
//             _ => unreachable!(),
//         };

//         match self
//             .send(
//                 poise::CreateReply::default().ephemeral(true).embed(
//                     serenity::CreateEmbed::default()
//                         .colour(constants::RED)
//                         .title("An Error Occurred!")
//                         .author(serenity::CreateEmbedAuthor::new(name).icon_url(avatar_url))
//                         .description(error_message)
//                         .footer(serenity::CreateEmbedFooter::new(format!(
//                             "Support Server: {}",
//                             self.data().config.main_server_invite
//                         ))),
//                 ),
//             )
//             .await
//         {
//             Ok(handle) => Ok(Some(handle)),
//             Err(_) => Ok(None),
//         }
//     }
// }

///Struct to represent everything needed to join a voice call.
pub struct JoinVCToken(pub serenity::GuildId, pub Arc<tokio::sync::Mutex<()>>);
impl JoinVCToken {
    pub fn acquire(data: &Data, guild_id: serenity::GuildId) -> Self {
        let lock = data
            .join_vc_tokens
            .entry(guild_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();

        Self(guild_id, lock)
    }
}

/// Extension trait for Songbird.
pub trait SongbirdManagerExt {
    fn join_vc(
        &self,
        guild_id: JoinVCToken,
        channel_id: serenity::ChannelId,
    ) -> impl Future<Output = Result<Arc<tokio::sync::Mutex<songbird::Call>>, songbird::error::JoinError>>;
}

/// Implementation of the extension trait for Songbird's manager.
impl SongbirdManagerExt for songbird::Songbird {
    async fn join_vc(
        &self,
        JoinVCToken(guild_id, lock): JoinVCToken,
        channel_id: serenity::ChannelId,
    ) -> Result<Arc<tokio::sync::Mutex<songbird::Call>>, songbird::error::JoinError> {
        let _guard = lock.lock().await;
        match self.join(guild_id, channel_id).await {
            Ok(call) => Ok(call),
            Err(err) => {
                // On error, the Call is left in a semi-connected state.
                // We need to correct this by removing the call from the manager.
                drop(self.leave(guild_id).await);
                Err(err)
            },
        }
    }
}

use poise::serenity_prelude::Context as SerenityContext;
use std::collections::HashSet;
pub fn check_bot_message(_serenity_ctx: &SerenityContext, msg: &Message) -> bool {
    let allowed_bots = HashSet::from([1111844110597374042, 1124707756750934159]);
    let author_id = msg.author.id;
    allowed_bots.contains(&author_id.get())
}
