use crate::{
    commands::{
        cmd_check_music,
        music::{create_skip_response, force_skip_top_track},
    },
    connection::get_voice_channel_for_user,
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    music::PlaybackOwner,
    poise_ext::{ContextExt, PoiseContextExt},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use serenity::Mentionable;

/// Vote to skip the current track
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn voteskip(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    voteskip_internal(ctx).await
}

/// Internal function for voteskip
#[cfg(not(tarpaulin_include))]
async fn voteskip_internal(ctx: Context<'_>) -> Result<(), Error> {
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
    let manager = ctx.data().songbird.clone();
    let call = manager.get(guild_id).unwrap();

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    let data = ctx.data();
    let mut cache_map = data.guild_cache_map.lock().await;
    // let cache_map = data.get_mut::<GuildCacheMap>().unwrap();

    let cache = cache_map.entry(guild_id).or_default();
    let user_id = ctx.get_user_id();
    cache.current_skip_votes.insert(user_id);

    let guild_users = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .unwrap()
        .clone()
        .voice_states;
    let channel_guild_users = guild_users
        .iter()
        .filter(|v| v.channel_id.unwrap() == bot_channel_id);
    let skip_threshold = channel_guild_users.count() / 2;

    let _ = if cache.current_skip_votes.len() >= skip_threshold {
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
        force_skip_top_track(&guard, &handler).await?;
        // The guard is held only for the mutation above, not across the
        // Discord round trip in `create_skip_response` -- see lease.rs.
        drop(guard);
        create_skip_response(ctx, &handler, 1).await
    } else {
        // Never mutates the queue on this path, so the guard is dropped
        // before the Discord round trip below rather than held idle across
        // it -- see lease.rs.
        drop(guard);
        ctx.send_reply_embed(CrackedMessage::VoteSkip {
            mention: ctx.get_user_id().mention(),
            missing: skip_threshold - cache.current_skip_votes.len(),
        })
        .await?
        .into_message()
        .await
        .map_err(|e| e.into())
    }?;
    Ok(())
}
