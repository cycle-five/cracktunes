use self::serenity::{model::id::GuildId, Mentionable};
use crate::{
    commands::music::{create_skip_response, force_skip_top_track},
    connection::get_voice_channel_for_user,
    errors::{verify, CrackedError},
    guild::cache::GuildCacheMap,
    messaging::message::CrackedMessage,
    utils::{get_user_id, send_response_poise_text},
    Context, Data, Error,
};
use poise::serenity_prelude as serenity;
use std::collections::HashSet;

/// Vote to skip the current track
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn voteskip(ctx: Context<'_>) -> Result<(), Error> {
    // use crate::db::TrackReaction;

    let guild_id = ctx.guild_id().unwrap();
    let guild = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .unwrap()
        .clone();
    let bot_channel_id =
        get_voice_channel_for_user(&guild, &ctx.serenity_context().cache.current_user().id)
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
        .voice_states
        .clone();
    let channel_guild_users = guild_users
        .into_values()
        .filter(|v| v.channel_id.unwrap() == bot_channel_id);
    let skip_threshold = channel_guild_users.count() / 2;

    let msg = if cache.current_skip_votes.len() >= skip_threshold {
        // // Write the skip votes to the db
        // TrackReaction::insert(
        //     &ctx.data().database_pool,
        //     TrackReaction {
        //         guild_id: guild_id.0 as i64,
        //         track_id: queue.current().unwrap().metadata().await.unwrap().track_id,
        //         reaction_type: "skip".to_string(),
        //         user_id: user_id.0 as i64,
        //     },
        // );
        force_skip_top_track(&handler).await?;
        create_skip_response(ctx, &handler, 1).await
    } else {
        send_response_poise_text(
            ctx,
            CrackedMessage::VoteSkip {
                mention: get_user_id(&ctx).mention(),
                missing: skip_threshold - cache.current_skip_votes.len(),
            },
        )
        .await
    }?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}

/// Forget all skip votes for a guild
// This is used when a track ends, or when a user leaves the voice channel.
// This is to prevent users from voting to skip a track, then leaving the voice channel.
// TODO: Should this be moved to a separate module? Or should it be moved to a separate file?
pub async fn forget_skip_votes(data: &Data, guild_id: GuildId) -> Result<(), Error> {
    let _res = data
        .guild_cache_map
        .lock()
        .await
        .entry(guild_id)
        .and_modify(|cache| cache.current_skip_votes = HashSet::new())
        .or_default();

    Ok(())
}
