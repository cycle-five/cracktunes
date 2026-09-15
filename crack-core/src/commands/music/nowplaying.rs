use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::status::{
    now_playing_pointer, pointer_goes_first, reply_floor, reply_privately, show_now_playing,
    show_now_playing_after,
};
use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::utils::get_track_handle_metadata;
use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    Context, Error,
};
use poise::CreateReply;
use serenity::all::CreateAllowedMentions;

/// Get the currently playing track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only,
    aliases("np")
)]
pub async fn nowplaying(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    nowplaying_internal(ctx).await
}

/// `/nowplaying`'s one-line reply, for both orderings.
///
/// 🔑 Mentions are suppressed (an empty allow-list parses none). The pointer
/// is message content carrying a track title, and the title is whatever the
/// uploader chose: `@everyone` or `<@&role>` in it would otherwise ping. The
/// embed this replaced never could. Only this reply opts out -- other commands
/// may mention on purpose, so there is no global default.
fn pointer_reply(content: String, private: bool) -> CreateReply<'static> {
    CreateReply::default()
        .content(content)
        .ephemeral(private)
        .allowed_mentions(CreateAllowedMentions::new())
}

/// Get the currently playing track. Internal function.
///
/// The status message shows the track; this replies with a one-line pointer
/// to it (spec: `/nowplaying` ordering).
pub async fn nowplaying_internal(ctx: Context<'_>) -> Result<(), Error> {
    let call = ctx.get_call().await?;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // 🔑 The Call lock is released at the end of this statement: the status
    // update below takes it.
    let track = call
        .lock()
        .await
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;
    let title = get_track_handle_metadata(&track)
        .await
        .ok()
        .and_then(|meta| meta.title)
        .unwrap_or_default();

    let data = ctx.data();
    let private = reply_privately(data.get_ephemeral_replies(guild_id).await, ctx.is_prefix());
    let music_channel = data.get_music_channel(guild_id).await;
    let serenity_ctx = ctx.serenity_context();

    if pointer_goes_first(private, music_channel, ctx.channel_id()) {
        let reply = ctx
            .send(pointer_reply(now_playing_pointer(&title, None), private))
            .await?;
        // 🔑 The reply's gateway echo almost never reaches the cache in the
        // few milliseconds before the status reads it, so the reply itself is
        // the floor: without it the status is edited in place above the "↓".
        // This branch only runs for a visible reply (`pointer_goes_first`).
        let after = reply_floor(&reply).await;
        show_now_playing_after(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
            after,
        )
        .await;
    } else {
        let shown = show_now_playing(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
        let link = shown.map(|status| status.id.link(status.channel, Some(guild_id)));
        ctx.send(pointer_reply(now_playing_pointer(&title, link), private))
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::CreateInteractionResponseMessage;

    /// The part of a reply's wire form this test reads.
    #[derive(serde::Deserialize)]
    struct WireReply {
        #[serde(default)]
        allowed_mentions: Option<WireMentions>,
    }

    #[derive(serde::Deserialize)]
    struct WireMentions {
        parse: Vec<String>,
        users: Vec<String>,
        roles: Vec<String>,
    }

    /// 🔑 The pointer carries a track title, which anyone uploading a video
    /// chooses. As message content -- unlike the embed it replaced -- an
    /// `@everyone` or `<@&role>` in it would ping, unless the reply says which
    /// mentions to parse: none.
    ///
    /// poise keeps `CreateReply::allowed_mentions` crate-private, so this reads
    /// it the way poise sends it: converted into serenity's builder and
    /// serialized.
    #[test]
    fn the_pointer_reply_never_pings() {
        for private in [false, true] {
            let reply = pointer_reply(now_playing_pointer("@everyone <@&1> <@2>", None), private);
            let wire = serde_json::to_string(
                &reply.to_slash_initial_response(CreateInteractionResponseMessage::new()),
            )
            .unwrap();
            let wire: WireReply = serde_json::from_str(&wire).unwrap();

            let mentions = wire
                .allowed_mentions
                .expect("the pointer must say which mentions to parse");
            assert!(mentions.parse.is_empty(), "parses {:?}", mentions.parse);
            assert!(mentions.users.is_empty());
            assert!(mentions.roles.is_empty());
        }
    }
}
