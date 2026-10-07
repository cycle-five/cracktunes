use self::serenity::builder::CreateEmbed;
use crate::commands::{cmd_check_music, help};
use crate::errors::CrackedError;
use crate::messaging::{courier, message::CrackedMessage};
use crate::music::ops::{self, OpCx, OpRefused, VolumeSet};
use crate::{Context, Error};
use poise::serenity_prelude as serenity;

/// Get or set the volume of the bot.
#[cfg(not(tarpaulin_include))]
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

#[cfg(not(tarpaulin_include))]
/// Internal method to handle volume changes.
pub async fn volume_internal(ctx: Context<'_>, level: Option<u32>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    let embed = match level {
        None => match ops::volume_now(&cx).await {
            Ok(now) => CreateEmbed::default().description(format!(
                "Current volume is {:.0}% in settings, {:.0}% in track.",
                now.setting * 100.0,
                now.track * 100.0
            )),
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
    courier::reply(ctx, CrackedMessage::CreateEmbed(Box::new(embed))).await?;
    Ok(())
}

fn not_connected_embed<'a>() -> CreateEmbed<'a> {
    CreateEmbed::default().description(format!("{}", CrackedError::NotConnected))
}

pub fn create_volume_embed<'a>(old: f32, new: f32) -> CreateEmbed<'a> {
    CreateEmbed::default().description(create_volume_desc(old, new))
}

pub fn create_volume_desc(old: f32, new: f32) -> String {
    VolumeSet { old, new }.description()
}
