use crate::commands::{cmd_check_music, help};
use crate::errors::CrackedError;
use crate::messaging::{courier, message::CrackedMessage};
use crate::music::ops::{self, OpCx, OpRefused, VolumeSet};
use crate::{Context, Error};
use crack_model::Card;

/// Get or set the volume of the bot.
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    aliases("vol"),
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Set the volume of the bot"] level: Option<u32>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    volume_internal(ctx, level).await
}

/// Internal method to handle volume changes.
pub async fn volume_internal(ctx: Context<'_>, level: Option<u32>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let embed = match level {
        None => match ops::volume_now(&cx).await {
            Ok(now) => current_volume_card(now.setting, now.track),
            Err(OpRefused::NotConnected) => not_connected_embed(),
            Err(refused) => return Err(CrackedError::from(refused).into()),
        },
        Some(percent) => match ops::volume(&cx, percent).await {
            Ok(done) => {
                let set = done.settle_now(&cx).await;
                create_volume_embed(set.old, set.new)
            },
            Err(OpRefused::NotConnected) => not_connected_embed(),
            Err(refused) => return Err(CrackedError::from(refused).into()),
        },
    };
    courier::reply(ctx, CrackedMessage::Card(Box::new(embed))).await?;
    Ok(())
}

fn current_volume_card(setting: f32, track: f32) -> Card {
    Card {
        description: Some(format!(
            "Current volume is {:.0}% in settings, {:.0}% in track.",
            setting * 100.0,
            track * 100.0
        )),
        ..Card::default()
    }
}

fn not_connected_embed() -> Card {
    Card {
        description: Some(CrackedError::NotConnected.to_string()),
        ..Card::default()
    }
}

pub fn create_volume_embed(old: f32, new: f32) -> Card {
    Card {
        description: Some(create_volume_desc(old, new)),
        ..Card::default()
    }
}

pub fn create_volume_desc(old: f32, new: f32) -> String {
    VolumeSet { old, new }.description()
}

#[cfg(test)]
mod pins {
    use super::*;
    #[test]
    fn the_current_volume_embed_is_byte_stable() {
        assert_eq!(
            serde_json::to_string(&crate::messaging::discord::embed(&current_volume_card(
                0.5, 0.8
            )))
            .unwrap(),
            r#"{"type":"rich","description":"Current volume is 50% in settings, 80% in track."}"#
        );
    }

    #[test]
    fn the_embeds_are_byte_stable() {
        assert_eq!(
            serde_json::to_string(&crate::messaging::discord::embed(&create_volume_embed(
                0.5, 0.8
            )))
            .unwrap(),
            r#"{"type":"rich","description":"Volume changed from 50% to 80%"}"#
        );
        assert_eq!(
            serde_json::to_string(&crate::messaging::discord::embed(&not_connected_embed()))
                .unwrap(),
            r#"{"type":"rich","description":"⚠️ I'm not connected to any voice channel!"}"#
        );
    }
}
