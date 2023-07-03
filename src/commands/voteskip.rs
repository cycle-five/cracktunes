use self::serenity::{
    model::id::GuildId,
    {Mentionable, RwLock, TypeMap},
};
use crate::{
    commands::{create_skip_response, force_skip_top_track},
    connection::get_voice_channel_for_user,
    errors::{verify, CrackedError},
    guild::cache::GuildCacheMap,
    messaging::message::CrackedMessage,
    utils::{create_response_poise_text, get_guild_id, get_user_id},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use std::{collections::HashSet, sync::Arc};

/// Vote to skip the current track
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn voteskip(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let bot_channel_id = get_voice_channel_for_user(
        &ctx.serenity_context().cache.guild(guild_id).unwrap(),
        &ctx.serenity_context().cache.current_user_id(),
    )
    .unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    let mut data = ctx.serenity_context().data.write().await;
    let cache_map = data.get_mut::<GuildCacheMap>().unwrap();

    let cache = cache_map.entry(guild_id).or_default();
    let user_id = get_user_id(&ctx);
    cache.current_skip_votes.insert(user_id);

    let guild_users = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .unwrap()
        .voice_states;
    let channel_guild_users = guild_users
        .into_values()
        .filter(|v| v.channel_id.unwrap() == bot_channel_id);
    let skip_threshold = channel_guild_users.count() / 2;

    if cache.current_skip_votes.len() >= skip_threshold {
        force_skip_top_track(&handler).await?;
        create_skip_response(ctx, &handler, 1).await
    } else {
        create_response_poise_text(
            &ctx,
            CrackedMessage::VoteSkip {
                mention: get_user_id(&ctx).mention(),
                missing: skip_threshold - cache.current_skip_votes.len(),
            },
        )
        .await
    }
}

pub async fn forget_skip_votes(data: &Arc<RwLock<TypeMap>>, guild_id: GuildId) -> Result<(), ()> {
    let mut data = data.write().await;

    let cache_map = data.get_mut::<GuildCacheMap>().ok_or(())?;
    let cache = cache_map.get_mut(&guild_id).ok_or(())?;
    cache.current_skip_votes = HashSet::new();

    Ok(())
}
