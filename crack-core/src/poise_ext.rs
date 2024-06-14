// use std::{borrow::Cow, future::Future, sync::Arc};

// use poise::serenity_prelude as serenity;

// use crate::{
//     //constants,
//     //constants::{FREE_NEUTRAL_COLOUR, PREMIUM_NEUTRAL_COLOUR},
//     //opt_ext::OptionTryUnwrap,
//     messaging::message::CrackedMessage,
//     require_guild,
//     utils::{self, OptionTryUnwrap},
//     Context,
// };
// use anyhow::Result as CrackedResult; //structs::{Context, JoinVCToken, Result, TTSMode},

// pub trait PoiseContextExt<'ctx> {
//     async fn send_error(
//         &'ctx self,
//         error_message: impl Into<Cow<'ctx, str>>,
//     ) -> CrackedResult<Option<poise::ReplyHandle<'ctx>>>;
//     async fn send_ephemeral(
//         &'ctx self,
//         message: impl Into<Cow<'ctx, str>>,
//     ) -> CrackedResult<poise::ReplyHandle<'ctx>>;

//     // async fn neutral_colour(&self) -> u32;
//     fn author_vc(&self) -> Option<serenity::ChannelId>;
//     async fn author_permissions(&self) -> CrackedResult<serenity::Permissions>;
// }

// impl<'ctx> PoiseContextExt<'ctx> for Context<'ctx> {
//     fn author_vc(&self) -> Option<serenity::ChannelId> {
//         require_guild!(self, None)
//             .voice_states
//             .get(&self.author().id)
//             .and_then(|vc| vc.channel_id)
//     }

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

//     async fn author_permissions(&self) -> CrackedResult<serenity::Permissions> {
//         // Handle non-guild call first, to allow try_unwrap calls to be safe.
//         if self.guild_id().is_none() {
//             return Ok(((serenity::Permissions::from_bits_truncate(
//                 0b111_1100_1000_0000_0000_0111_1111_1000_0100_0000,
//             ) | serenity::Permissions::SEND_MESSAGES)
//                 - serenity::Permissions::SEND_TTS_MESSAGES)
//                 - serenity::Permissions::MANAGE_MESSAGES);
//         }

//         // Accesses guild cache and is asynchronous, must be called first.
//         let member = self.author_member().await.try_unwrap()?;

//         // Accesses guild cache, but the member above was cloned out, so safe.
//         let guild = self.guild().try_unwrap()?;

//         // Does not access cache, but relies on above guild cache reference.
//         let channel = guild.channels.get(&self.channel_id()).try_unwrap()?;

//         // Does not access cache.
//         Ok(guild.user_permissions_in(channel, &member))
//     }

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

// // pub trait SongbirdManagerExt {
// //     async fn join_vc(
// //         &self,
//         guild_id: JoinVCToken,
//         channel_id: serenity::ChannelId,
//     ) -> Result<Arc<tokio::sync::Mutex<songbird::Call>>, songbird::error::JoinError>;
// }

// impl SongbirdManagerExt for songbird::Songbird {
//     async fn join_vc(
//         &self,
//         JoinVCToken(guild_id, lock): JoinVCToken,
//         channel_id: serenity::ChannelId,
//     ) -> Result<Arc<tokio::sync::Mutex<songbird::Call>>, songbird::error::JoinError> {
//         let _guard = lock.lock().await;
//         match self.join(guild_id, channel_id).await {
//             Ok(call) => Ok(call),
//             Err(err) => {
//                 // On error, the Call is left in a semi-connected state.
//                 // We need to correct this by removing the call from the manager.
//                 drop(self.leave(guild_id).await);
//                 Err(err)
//             },
//         }
//     }
// }
