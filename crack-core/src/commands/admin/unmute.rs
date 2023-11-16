use serenity::all::GuildId;
use serenity::async_trait;
use serenity::builder::EditMember;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Unmute a user.
/// TODO: Add a way to unmute a user by their ID.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn unmute(
    ctx: Context<'_>,
    #[description = "User of unmute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    match get_guild_id(ctx) {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(ctx, user.clone().id, EditMember::new().mute(false))
                .await
            {
                // Handle error, send error message
                send_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to unmute user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                send_response_poise(
                    ctx,
                    CrackedMessage::UserUnmuted {
                        user: user.name.clone(),
                        user_id: user.clone().id,
                    },
                )
                .await?;
            }
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    }
    Ok(())
}

#[async_trait]
pub trait ContextMock {
    fn guild_id(&self) -> Option<GuildId>;
    fn edit_member(
        &self,
        user_id: serenity::model::id::UserId,
        edit_member: EditMember,
    ) -> Result<(), serenity::Error>;
}

#[async_trait]
impl<U, E> ContextMock for poise::Context<'_, U, E> {
    fn guild_id(&self) -> Option<GuildId> {
        Some(GuildId::new(1))
    }

    fn edit_member(
        &self,
        _user_id: serenity::model::id::UserId,
        _edit_member: EditMember,
    ) -> Result<(), serenity::Error> {
        Ok(())
    }
}

fn get_guild_id(ctx: impl ContextMock) -> Option<GuildId> {
    ctx.guild_id()
}

// fn edit_member(
//     guild_id: GuildId,
//     ctx: impl ContextMock + CacheHttp,
//     user_id: serenity::model::id::UserId,
//     edit_member: EditMember,
// ) -> Result<(), SerenityError> {
//     guild_id.edit_member(ctx, user_id, edit_member)
// }

// fn send_response_poise(
//     ctx: impl ContextMock,
//     message: CrackedMessage,
// ) -> Result<(), SerenityError> {
//     tracing::warn!("4", message);
// }

// mod test {
//     use serenity::async_trait;

//     use crate::{commands::unmute, Context};

//     #[test]
//     fn test_unmute() {
//         unmute(ctx, user).unwrap();

//         // mockall::mock! {
//         //     serenity::model::user::User {
//         //         fn name(&self) -> String;
//         //         fn id(&self) -> serenity::model::id::UserId;
//         //     }
//         // }

//         // mockall::mock! {
//         //     serenity::model::guild::Guild {
//         //         fn edit_member(&self, ctx: &Context, user_id: serenity::model::id::UserId, edit_member: serenity::builder::EditMember) -> Result<(), serenity::Error>;
//         //     }
//         // }

//         // mockall::mock! {
//         //     serenity::builder::EditMember {
//         //         fn mute(&mut self, mute: bool) -> &mut Self;
//         //     }
//         // }
//     }
// }
