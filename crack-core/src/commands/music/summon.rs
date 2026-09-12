use crate::commands::{cmd_check_music, connected_call, do_join, help, sub_help as help};
use crate::{
    connection::get_voice_channel_for_user_summon, errors::CrackedError,
    messaging::message::CrackedMessage, poise_ext::ContextExt, poise_ext::PoiseContextExt, Context,
    Error,
};
use ::serenity::all::{Channel, ChannelId, GenericChannelId, Mentionable};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Summon the bot to your voice channel.
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    aliases("join", "come here", "comehere", "come", "here"),
    guild_only
)]
pub async fn summon(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    summon_internal(ctx, None, None).await
}

/// Summon a bot to a specific voice channel.
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    check = "cmd_check_music",
    subcommands("help"),
    guild_only
)]
pub async fn summonchannel(
    ctx: Context<'_>,
    #[description = "Channel to summon the bot to."] channel: Option<Channel>,
    #[description = "Channel Id of the channel to summon the bot to."] channel_id_str: Option<
        String,
    >,
) -> Result<(), Error> {
    summon_internal(ctx, channel, channel_id_str).await
}

/// Internal method to handle summonging the bot to a voice channel.
pub async fn summon_internal(
    ctx: Context<'_>,
    channel: Option<Channel>,
    channel_id_str: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let manager = ctx.data().songbird.clone();
    let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?.clone();
    let user_id = ctx.get_user_id();

    let channel_id = match parse_channel_id(channel, channel_id_str)? {
        Some(id) => id,
        None => get_voice_channel_for_user_summon(&guild, &user_id)?,
    };

    // 🪤 This must ask for a *connected* call, not merely a registered one.
    // songbird keeps the Call after a failed join or a forced disconnect, and
    // the old `manager.get` accepted it: the `_ => call.clone()` arm below
    // returned that dead handle, so `do_join` never ran, `ensure_can_join`
    // never ran, no join was attempted and the command returned Ok having
    // sent nothing at all.
    let call: Arc<Mutex<Call>> = match connected_call(&manager, guild_id, None).await {
        Some(call) => {
            let here = call.lock().await.current_channel();
            match here {
                // 🪤 This arm used to be unreachable, and its absence was
                // user-visible. songbird reads `current_connection()` and
                // `current_channel()` from the same field, so a *connected*
                // Call always has a channel -- which meant the arm below fired
                // for every connected case, including a `/summon` into the
                // channel the bot was already sitting in. The user got a red
                // "⚠️ I'm already connected to <#X>" naming the channel they
                // had just asked for (#506).
                Some(here) if here.get() == channel_id.get() => {
                    // Answer rather than falling through to `Ok(())` having
                    // sent nothing: a command that returns without replying is
                    // "The application did not respond", which is the failure
                    // #499 fixed on this exact path. `/summon` into the channel
                    // we are already in is a no-op, and a no-op still owes the
                    // user a sentence.
                    ctx.send_reply_embed(CrackedMessage::Summon {
                        mention: channel_id.mention(),
                    })
                    .await?;
                    call
                },
                // Genuinely somewhere else -- the case this error is for.
                Some(here) => {
                    return Err(CrackedError::AlreadyConnected(
                        GenericChannelId::new(here.get()).mention(),
                    )
                    .into())
                },
                // Not reachable today, for the reason above. "Connected to
                // nowhere" is still not something to report as
                // already-connected, so treat it as needing a join.
                None => do_join(ctx, &manager, guild_id, channel_id).await?,
            }
        },
        None => do_join(ctx, &manager, guild_id, channel_id).await?,
    };
    let chan_id = call
        .lock()
        .await
        .current_channel()
        .map(|c| GenericChannelId::new(c.get()));
    if let Some(c) = chan_id {
        tracing::info!("joined channel: {c}");
    } else {
        tracing::warn!("Not in channel after join?!?");
    }
    Ok(())
}

/// Internal method to parse the channel id.
fn parse_channel_id(
    channel: Option<Channel>,
    channel_id_str: Option<String>,
) -> Result<Option<ChannelId>, Error> {
    if let Some(channel) = channel {
        return Ok(Some(channel.id().expect_channel()));
    }

    match channel_id_str {
        Some(id) => {
            tracing::warn!("channel_id_str: {:?}", id);
            match id.parse::<u64>() {
                Ok(id) => Ok(Some(ChannelId::new(id))),
                Err(e) => Err(e.into()),
            }
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod test {
    use crate::commands::music::summon::parse_channel_id;
    use serenity::model::id::ChannelId;

    #[test]
    fn test_parse_channel_id() {
        let channel = None;

        assert_eq!(parse_channel_id(channel, None).unwrap(), None);
        assert_eq!(
            parse_channel_id(None, Some("123".to_string())).unwrap(),
            Some(ChannelId::new(123))
        );
        assert!(parse_channel_id(None, Some("abc".to_string())).is_err());
        assert_eq!(parse_channel_id(None, None).unwrap(), None);
    }
}
