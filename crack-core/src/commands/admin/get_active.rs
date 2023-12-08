use crate::Context;
use crate::Error;
use serenity::all::ChannelId;
use serenity::all::ChannelType;
use serenity::all::GuildChannel;
use serenity::all::GuildId;
use serenity::all::UserId;
use std::collections::HashMap;

/// Get list of VCs the bot is currently in.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn get_active_vcs(ctx: Context<'_>) -> Result<(), Error> {
    // Get all guilds the bot is in
    let mut active_vcs = Vec::new();
    let guilds = get_guilds(ctx).await?;
    let user_id = ctx.serenity_context().cache.current_user().id;
    for (guild_id, _, _) in guilds.iter() {
        let guild_channels = guild_id.channels(ctx).await?;
        let vcs = get_active_vcs_impl(ctx, guild_channels, user_id).await?;
        active_vcs.extend(vcs);
    }
    poise::say_reply(ctx, format!("Active VCs: {:?}", active_vcs)).await?;
    Ok(())
}

/// Get list of guilds the bot is in.
#[cfg(not(tarpaulin_include))]
pub async fn get_guilds(ctx: Context<'_>) -> Result<Vec<(GuildId, String, u64)>, Error> {
    let mut hidden_guilds = 0;
    let mut guilds = Vec::<(GuildId, String, u64)>::new();
    for guild_id in ctx.cache().guilds() {
        match ctx.cache().guild(guild_id) {
            Some(guild) => guilds.push((guild.id, guild.name.clone(), guild.member_count)),
            None => hidden_guilds += 1, // uncached guild
        }
    }
    tracing::warn!("Hidden guilds: {}", hidden_guilds);
    // let guild_channels = ctx.guild_id().unwrap().channels(ctx).await?;
    // let bot_id = ctx.serenity_context().cache.current_user().id;
    guilds.sort_by_key(|(_, _, member)| u64::MAX - member); // sort largest guilds first
    Ok(guilds)
}

/// Get list of VCs the bot is currently in.
#[cfg(not(tarpaulin_include))]
pub async fn get_active_vcs_impl(
    ctx: Context<'_>,
    guild_channels: HashMap<ChannelId, GuildChannel>,
    bot_id: UserId,
) -> Result<Vec<(GuildId, String)>, Error> {
    let mut active_vcs = Vec::new();

    for (_channel_id, channel) in guild_channels {
        if channel.kind == ChannelType::Voice {
            let members = channel.members(ctx)?;
            for member in members {
                if bot_id == member.user.id {
                    active_vcs.push((channel.guild_id, channel.name.clone()));
                }
            }
        }
    }

    Ok(active_vcs)
}
