use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::courier;
use crate::messaging::format::{TrackLabel, INLINE_TITLE_MAX};
use crate::messaging::render::Rendered;
use crate::messaging::status::{
    now_playing_pointer, pointer_goes_first, reply_privately, show_now_playing,
    show_now_playing_after,
};
use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::utils::get_track_handle_metadata;
use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    Context, Error,
};
use serenity::all::MessageLink;
use songbird::input::AuxMetadata;

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

/// `/nowplaying`'s one-line reply, as plain message content (v0.13.0 replaced
/// an embed with it on purpose).
///
/// 🔑 Mentions are suppressed: `Rendered::text` defaults to `Mentions::None`,
/// an empty allow-list. The pointer carries a track title, and the title is
/// whatever the uploader chose: `@everyone` or `<@&role>` in it would
/// otherwise ping.
fn pointer_reply(content: String) -> Rendered {
    Rendered::text(content)
}

/// `/nowplaying`'s one-line pointer text. The title is whatever the uploader
/// chose, so it arrives escaped (`a*b` must not turn the rest bold) and
/// `(untitled)` when blank.
///
/// 🔑 It is sent as `CrackedMessage::Other`, which renders with no mentions
/// allowed: `@everyone` in a title cannot ping.
fn pointer_text(meta: &AuxMetadata, link: Option<MessageLink>) -> String {
    now_playing_pointer(
        &TrackLabel::from_metadata(meta).title_text(INLINE_TITLE_MAX),
        link,
    )
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
    // A track without metadata still gets a pointer, just an untitled one --
    // but not silently.
    let meta = match get_track_handle_metadata(&track).await {
        Ok(meta) => meta,
        Err(err) => {
            tracing::warn!("nowplaying: no metadata for the current track in {guild_id}: {err}");
            AuxMetadata::default()
        },
    };

    let data = ctx.data();
    let private = reply_privately(data.get_ephemeral_replies(guild_id).await, ctx.is_prefix());
    let music_channel = data.get_music_channel(guild_id).await;
    let serenity_ctx = ctx.serenity_context();

    if pointer_goes_first(private, music_channel, ctx.channel_id()) {
        let reply =
            courier::reply_rendered(ctx, pointer_reply(pointer_text(&meta, None)), private).await?;
        // 🔑 The reply's gateway echo almost never reaches the cache in the
        // few milliseconds before the status reads it, so the reply itself is
        // the floor: without it the status is edited in place above the "↓".
        // This branch only runs for a visible reply (`pointer_goes_first`).
        let after = courier::locate(ctx, &reply).await;
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
        courier::reply_rendered(ctx, pointer_reply(pointer_text(&meta, link)), private).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titled(title: &str) -> AuxMetadata {
        AuxMetadata {
            title: Some(title.to_owned()),
            ..Default::default()
        }
    }

    /// A title is third-party text: its markdown must not leak into the pointer.
    #[test]
    fn the_pointer_escapes_the_title() {
        let text = pointer_text(&titled("a*b"), None);
        assert!(text.contains("a\\*b"), "{text}");
    }

    /// 🔑 Plain content, never an embed, and it pings nobody.
    #[test]
    fn the_pointer_is_plain_content_that_never_pings() {
        let text = pointer_text(&titled("@everyone <@&1> <@2>"), None);
        let reply = pointer_reply(text.clone());
        assert!(reply.embed.is_none());
        assert_eq!(reply.content.as_deref(), Some(text.as_str()));
        let am = serde_json::to_value(reply.allowed_mentions()).unwrap();
        assert_eq!(am["parse"], serde_json::json!([]));
    }

    #[test]
    fn a_track_without_a_title_is_pointed_to_as_untitled() {
        let text = pointer_text(&AuxMetadata::default(), None);
        assert!(text.contains("**(untitled)**"), "{text}");
    }
}
