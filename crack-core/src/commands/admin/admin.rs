use poise::serenity_prelude::Channel;

use crate::{guild::settings::WelcomeSettings, Context, Error};
// use chrono::NaiveTime;
// use date_time_parser::TimeParser;

/// Admin commands.
#[poise::command(
    prefix_command,
    //slash_command,
    subcommands(
    ),
    ephemeral,
    owners_only
)]
pub async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    Ok(())
}

//
// There are user management admin commands
//

// /// Create category.
// #[poise::command(prefix_command, , owners_only, ephemeral)]
// pub async fn create_category(ctx: Context<'_>, category_name: String) -> Result<(), Error> {
//     match ctx.guild_id() {
//         Some(guild) => {
//             let guild = guild.to_partial_guild(&ctx).await?;
//             match guild
//                 .create_(&ctx, |c| {
//                     c.name(category_name)
//                         .kind(serenity::model::channel::ChannelType::Category)
//                 })
//                 .await
//             {
//                 Err(e) => {
//                     // Handle error, send error message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::Other(format!("Failed to create category: {}", e)),
//                     )
//                     .await?;
//                 }
//                 Some(category) => {
//                     // Send success message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::CategoryCreated {
//                             category_name: category_name.clone(),
//                         },
//                     )
//                     .await?;
//                 }
//             }
//         }
//         None => {
//             return Result::Err(
//                 CrackedError::Other("This command can only be used in a guild.").into(),
//             );
//         }
//     }
//     Ok(())
// }

// /// Delete role.
// #[poise::command(prefix_command, , owners_only, ephemeral)]
// pub async fn delete_role(ctx: Context<'_>, role_name: String) -> Result<(), Error> {
//     match ctx.guild_id() {
//         Some(guild) => {
//             let guild = guild.to_partial_guild(&ctx).await?;
//             let role = guild
//                 .roles(&ctx)
//                 .await?
//                 .into_iter()
//                 .find(|r| r.name == role_name);
//             if let Some(role) = role {
//                 if let Err(e) = role.delete(&ctx).await {
//                     // Handle error, send error message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::Other(format!("Failed to delete role: {}", e)),
//                     )
//                     .await?;
//                 } else {
//                     // Send success message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::RoleDeleted {
//                             role_name: role_name.clone(),
//                         },
//                     )
//                     .await?;
//                 }
//             } else {
//                 create_response_poise(ctx, CrackedMessage::Other("Role not found.".to_string()))
//                     .await?;
//             }
//         }
//         None => {
//             return Result::Err(
//                 CrackedError::Other("This command can only be used in a guild.".to_string()).into(),
//             );
//         }
//     }
//     Ok(())
// }

// /// Delete category.
// #[poise::command(prefix_command, , owners_only, ephemeral)]
// pub async fn delete_category(ctx: Context<'_>, category_name: String) -> Result<(), Error> {
//     match ctx.guild_id() {
//         Some(guild) => {
//             let guild = guild.to_partial_guild(&ctx).await?;
//             let category = guild
//                 .channels(&ctx)
//                 .await?
//                 .into_iter()
//                 .find(|c| c.name == category_name);
//             if let Some(category) = category {
//                 if let Err(e) = category.delete(&ctx).await {
//                     // Handle error, send error message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::Other(format!("Failed to delete category: {}", e)),
//                     )
//                     .await?;
//                 } else {
//                     // Send success message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::CategoryDeleted {
//                             category_name: category_name.clone(),
//                         },
//                     )
//                     .await?;
//                 }
//             } else {
//                 create_response_poise(
//                     ctx,
//                     CrackedMessage::Other("Category not found.".to_string()),
//                 )
//                 .await?;
//             }
//         }
//         None => {
//             return Result::Err(
//                 CrackedError::Other("This command can only be used in a guild.".to_string()).into(),
//             );
//         }
//     }
//     Ok(())
// }

// /// Delete role.
// #[poise::command(prefix_command, owners_only, ephemeral)]
// pub async fn delete_role_by_id(ctx: Context<'_>, role_id: u64) -> Result<(), Error> {
//     match ctx.guild_id() {
//         Some(guild) => {
//             let guild = guild.to_partial_guild(&ctx).await?;
//             let role = guild
//                 .roles(&ctx)
//                 .await?
//                 .into_iter()
//                 .find(|r| r.id == role_id);
//             if let Some(role) = role {
//                 if let Err(e) = role.delete(&ctx).await {
//                     // Handle error, send error message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::Other(format!("Failed to delete role: {}", e)),
//                     )
//                     .await?;
//                 } else {
//                     // Send success message
//                     create_response_poise(
//                         ctx,
//                         CrackedMessage::RoleDeleted {
//                             role_name: role.name.clone(),
//                         },
//                     )
//                     .await?;
//                 }
//             } else {
//                 create_response_poise(ctx, CrackedMessage::Other("Role not found.".to_string()))
//                     .await?;
//             }
//         }
//         None => {
//             return Result::Err(
//                 CrackedError::Other("This command can only be used in a guild.".to_string()).into(),
//             );
//         }
//     }
//     Ok(())
// }

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_welcome_settings(
    ctx: Context<'_>,
    #[description = "The channel to send welcome messages"] channel: Channel,
    #[description = "Welcome message template use {user} for username"] message: String,
) -> Result<(), Error> {
    let welcome_settings = WelcomeSettings {
        channel_id: Some(channel.id().get()),
        message: Some(message.clone()),
        auto_role: None,
    };
    let _res = ctx
        .data()
        .guild_settings_map
        .lock()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.welcome_settings = Some(welcome_settings.clone());
        });
    Ok(())
}
