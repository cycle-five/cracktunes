use std::{io::Write, sync::Arc};

use poise::{
    serenity_prelude::{Channel, CreateMessage, User, UserId},
    CreateReply,
};
use serenity::{
    all::{ChannelId, GuildId},
    builder::{CreateChannel, EditMember, EditRole},
};
use typemap_rev::TypeMap;

use crate::{
    errors::CrackedError,
    guild::settings::{GuildSettings, GuildSettingsMap, WelcomeSettings, DEFAULT_PREFIX},
    messaging::message::CrackedMessage,
    utils::{check_reply, create_response_poise, get_current_voice_channel_id, get_guild_name},
    Context, Data, Error,
};
// use chrono::NaiveTime;
// use date_time_parser::TimeParser;

/// Admin commands.
#[poise::command(
    prefix_command,
    //slash_command,
    subcommands(
        "authorize",
        "deauthorize",
        "broadcast_voice",
        "get_settings",
        "print_settings",
        "set_idle_timeout",
        "set_prefix",
        "set_join_leave_log_channel",
        "set_all_log_channel",
        "kick",
        "ban",
        "unban",
        "mute",
        "unmute",
        "deafen",
        "undeafen",
        "create_voice_channel",
        "create_text_channel",
        "delete_channel",
        "create_role",
        "audit_logs",
    ),
    ephemeral,
    owners_only,
    hide_in_help
)]
pub async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    Ok(())
}

/// Set the prefix for the bot.
#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn set_prefix(
    ctx: Context<'_>,
    #[description = "The prefix to set for the bot"] prefix: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let _entry = &data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.prefix = prefix.clone())
        .and_modify(|e| e.prefix_up = prefix.to_uppercase());

    let settings = data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .get_mut(&guild_id);

    let _res = settings.map(|s| s.save()).unwrap();

    create_response_poise(
        ctx,
        CrackedMessage::Other(format!("Prefix set to {}", prefix)),
    )
    .await?;

    Ok(())
}

/// Authorize a user to use the bot.
// #[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    let _res = data
        .guild_settings_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.authorized_users.insert(id);
            e.save().unwrap();
        })
        .key();

    //ctx.send("User authorized").await;
    check_reply(
        ctx.send(CreateReply::new().content("User authorized.").reply(true))
            .await,
    );

    Ok(())
}

/// Deauthorize a user from using the bot.
#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let mut guild_settings = data
        .guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id)
        .expect("Failed to get guild settings map")
        .clone();
    let res = guild_settings.authorized_users.remove(&id);
    guild_settings.save()?;

    if res {
        check_reply(
            ctx.send(CreateReply::new().content("User deauthorized.").reply(true))
                .await,
        );
        Ok(())
    } else {
        Err(CrackedError::UnauthorizedUser.into())
    }
}

/// Broadcast a message to all guilds where the bot is currently in a voice channel.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn broadcast_voice(
    ctx: Context<'_>,
    #[rest]
    #[description = "The message to broadcast"]
    message: String,
) -> Result<(), Error> {
    let data = ctx.data();
    let http = ctx.http();
    let serenity_ctx = ctx.serenity_context().clone();
    let guilds = data.guild_settings_map.lock().unwrap().clone();

    for (guild_id, _settings) in guilds.iter() {
        let message = message.clone();

        let channel_id_opt = get_current_voice_channel_id(&serenity_ctx, *guild_id).await;

        if let Some(channel_id) = channel_id_opt {
            channel_id
                .send_message(&http, CreateMessage::new().content(message.clone()))
                .await
                .unwrap();
        }
    }

    Ok(())
}

/// Set the idle timeout for the bot in vc.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn set_idle_timeout(
    ctx: Context<'_>,
    #[description = "Idle timeout for the bot in minutes."] timeout: u32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    // let timeout = match TimeParser::parse(&timeout) {
    //     Some(time) => time,
    //     None => return Err(CrackedError::ParseTimeFail.into()),
    // };
    // let timeout = timeout
    //     .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
    //     .num_seconds() as u32;
    let timeout = timeout * 60;

    data.guild_settings_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.timeout = timeout);

    check_reply(
        ctx.send(
            CreateReply::new()
                .content(format!("timeout set to {} seconds", timeout))
                .reply(true),
        )
        .await,
    );

    Ok(())
}

//
// There are user management admin commands
//

/// Kick command to kick a user from the server based on their ID
#[poise::command(prefix_command, hide_in_help, ephemeral, owners_only)]
pub async fn kick(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.kick(&ctx, user_id).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to kick user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(ctx, CrackedMessage::UserKicked { user_id }).await?;
            }
        }
        None => {
            create_response_poise(
                ctx,
                CrackedMessage::Other("This command can only be used in a guild.".to_string()),
            )
            .await?;
        }
    }
    Ok(())
}

/// Ban a user from the server.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn ban(
    ctx: Context<'_>,
    user: User,
    dmd: Option<u8>,
    reason: Option<String>,
) -> Result<(), Error> {
    let dmd = dmd.unwrap_or(0);
    let reason = reason.unwrap_or("No reason provided".to_string());
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.ban_with_reason(&ctx, user.clone(), dmd, reason).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to ban user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserBanned {
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

/// Mute a user.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn mute(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().mute(true))
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to mute user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserMuted {
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

/// Deafen a user.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn deafen(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().deafen(true))
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to deafen user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserMuted {
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

/// Undeafen a user.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn undeafen(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().deafen(false))
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to undeafen user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserMuted {
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

/// Retreive audit logs.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn audit_logs(ctx: Context<'_>) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let logs = guild.audit_logs(&ctx, None, None, None, None).await?;
            // open a file to write to
            let mut file = std::fs::File::create("audit_logs.txt")?;
            // write the logs to the file
            file.write_all(format!("{:?}", logs).as_bytes())?;
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    }
    Ok(())
}

/// Unban a user from the server.
/// TODO: Add a way to unban a user by their ID.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn unban(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.unban(&ctx, user.clone()).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to unban user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserUnbanned {
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

/// Unmute a user.]
/// TODO: Add a way to unmute a user by their ID.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn unmute(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().mute(false))
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to unmute user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
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

/// Create voice channel.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn create_voice_channel(ctx: Context<'_>, channel_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild
                .create_channel(
                    &ctx,
                    CreateChannel::new(channel_name.clone())
                        .kind(serenity::model::channel::ChannelType::Voice),
                )
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::VoiceChannelCreated {
                        channel_name: channel_name.clone(),
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

/// Create text channel.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn create_text_channel(ctx: Context<'_>, channel_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            match guild
                .create_channel(
                    &ctx,
                    CreateChannel::new(channel_name.clone())
                        .kind(serenity::model::channel::ChannelType::Voice),
                )
                .await
            {
                Err(e) => {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                    )
                    .await?;
                }
                Ok(channel) => {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::TextChannelCreated {
                            channel_name: channel.name.clone(),
                            channel_id: channel.id,
                        },
                    )
                    .await?;
                }
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

/// Set the join-leave log channel.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn set_join_leave_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send join/leave logs"] channel: Channel,
) -> Result<(), Error> {
    let channel_id = channel.id();
    let guild_id = ctx.guild_id().unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let _entry = &data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.set_join_leave_log_channel(channel_id.get()));

    let settings = data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .get_mut(&guild_id);

    let _res = settings.map(|s| s.save()).unwrap();

    create_response_poise(
        ctx,
        CrackedMessage::Other(format!("Join-leave log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}

pub async fn set_all_log_channel_data(
    data: Data,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<GuildSettings, Error> {
    //let mut data = map.write().await;
    //let entry = data
    Ok(data
        .guild_settings_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.set_all_log_channel(channel_id.into());
        })
        .or_insert({
            GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None)
                .set_all_log_channel(channel_id.into())
                .to_owned()
        })
        .to_owned())

    //Ok(entry.clone())
    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // create_response_poise(
    //     ctx,
    //     CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    // )
    // .await?;

    //    Ok(())
}

pub async fn set_all_log_channel_old_data(
    map: Arc<tokio::sync::RwLock<TypeMap>>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<GuildSettings, Error> {
    let mut data = map.write().await;
    //let entry = data
    Ok(data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.set_all_log_channel(channel_id.into());
        })
        .or_insert({
            GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None)
                .set_all_log_channel(channel_id.into())
                .to_owned()
        })
        .to_owned())

    //Ok(entry.clone())
    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // create_response_poise(
    //     ctx,
    //     CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    // )
    // .await?;

    //    Ok(())
}

/// Set the join-leave log channel.
#[poise::command(prefix_command, owners_only, hide_in_help)]
pub async fn set_all_log_channel(
    ctx: Context<'_>,
    #[description = "Channel to send all logs"] channel: Channel,
) -> Result<(), Error> {
    let channel_id = channel.id();
    let guild_id = ctx.guild_id().unwrap();
    // let mut data = ctx.serenity_context().data.write().await;
    // let data = &ctx.serenity_context().data;

    // set_all_log_channel_old_data(ctx.serenity_context().data.clone(), guild_id, channel_id).await?;
    set_all_log_channel_data(ctx.data().clone(), guild_id, channel_id).await?;
    // let _entry = &data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .entry(guild_id)
    //     .and_modify(|e| e.set_all_log_channel(channel_id.get()));

    // let settings = data
    //     .get_mut::<GuildSettingsMap>()
    //     .unwrap()
    //     .get_mut(&guild_id);

    // let _res = settings.map(|s| s.save()).unwrap();

    create_response_poise(
        ctx,
        CrackedMessage::Other(format!("all log channel set to {}", channel_id)),
    )
    .await?;

    Ok(())
}

// pub fn get_reply_handle(ctx: Context) -> ReplyHandle {
//     ctx.reply_handle()
// }

/// Get the current bot settings for this guild.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn get_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    {
        let settings_ro = {
            let mut guild_settings_map = ctx.data().guild_settings_map.lock().unwrap();
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id),
                ));
            settings.clone()
        };

        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings: {:?}", settings_ro)),
        )
        .await?;
    }

    Ok(())
}

/// Get the current bot settings for this guild.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn print_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_settings_map = ctx.data().guild_settings_map.lock().unwrap().clone();

    for (guild_id, settings) in guild_settings_map.iter() {
        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }

    let guild_settings_map = ctx.serenity_context().data.read().await;

    for (guild_id, settings) in guild_settings_map.get::<GuildSettingsMap>().unwrap().iter() {
        create_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }
    Ok(())
}

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

/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn create_role(ctx: Context<'_>, role_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            match guild
                .create_role(&ctx, EditRole::new().name(role_name))
                .await
            {
                Err(e) => {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to create role: {}", e)),
                    )
                    .await?;
                }
                Ok(role) => {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::RoleCreated {
                            role_name: role.name.clone(),
                            role_id: role.id,
                        },
                    )
                    .await?;
                }
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

/// Delete channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_channel(ctx: Context<'_>, channel_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let channel = guild
                .channels(&ctx)
                .await?
                .into_iter()
                .find(|(_channel_id, guild_chan)| guild_chan.name == channel_name);
            if let Some((channel_id, guild_chan)) = channel {
                if let Err(e) = guild_chan.delete(&ctx).await {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to delete channel: {}", e)),
                    )
                    .await?;
                } else {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::ChannelDeleted {
                            channel_id: channel_id,
                            channel_name: channel_name.clone(),
                        },
                    )
                    .await?;
                }
            } else {
                create_response_poise(ctx, CrackedMessage::Other("Channel not found.".to_string()))
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

#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
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
