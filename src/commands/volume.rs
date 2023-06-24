use self::serenity::builder::CreateEmbed;
use self::serenity::http::CacheHttp;
use crate::errors::CrackedError;
use crate::utils::{create_embed_response, create_embed_response_poise};
use crate::{Context, Data, Error};
use poise::{serenity_prelude as serenity, ApplicationCommandOrAutocompleteInteraction};
use songbird::tracks::TrackHandle;
use std::borrow::BorrowMut;
use std::sync::Arc;

#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "The volume to set the player to"] level: Option<u32>,
) -> Result<(), Error> {
    tracing::info!("volume");
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            let mut embed = CreateEmbed::default();
            embed.description(format!("{}", CrackedError::NotConnected));
            create_embed_response_poise(ctx, embed).await?;
            return Ok(());
        }
    };

    let old_ctx = ctx.serenity_context();

    let to_set = match level {
        Some(arg) => Some(arg as isize),
        None => {
            let handler = call.lock().await;
            let track_handle: Option<TrackHandle> = handler.queue().current();
            if track_handle.is_none() {
                let mut embed = CreateEmbed::default();
                embed.description(format!("{}", CrackedError::NothingPlaying));

                create_embed_response_poise(ctx, embed).await?;
                return Ok(());
            }
            let volume = track_handle.unwrap().get_info().await.unwrap().volume;
            let mut embed = CreateEmbed::default();
            embed.description(format!("Current volume is {}%", volume * 100.0));
            match ctx {
                Context::Prefix(prefix_ctx) => {
                    prefix_ctx
                        .msg
                        .reply(
                            old_ctx.http(),
                            format!("Current volume is {}%", volume * 100.0),
                        )
                        .await?;
                    // create_embed_response(&Arc::new(*old_ctx.http()), , embed).await?;
                }
                Context::Application(app_ctx) => match app_ctx.interaction {
                    ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => {
                        let mut interaction = x.clone();
                        create_embed_response(&old_ctx.http, interaction.borrow_mut(), embed)
                            .await?;
                    }
                    ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => {
                        panic!("Autocomplete not implemented");
                    }
                },
            }
            return Ok(());
        }
    };

    let handler = call.lock().await;
    let new_volume = to_set.unwrap() as f32 / 100.0;
    let track_handle: TrackHandle = handler.queue().current().expect("No track playing");
    let old_volume = track_handle.get_info().await.unwrap().volume;

    track_handle.set_volume(new_volume).unwrap();
    // lock the mutex inside of it's own block, so it isn't locked across an await.
    let _x = {
        let data: &Arc<Data> = ctx.data();
        let mut res = data.volume.lock().unwrap();
        *res = new_volume;
        *res
    };

    let embed = create_volume_embed(old_volume, new_volume);

    match ctx {
        Context::Prefix(prefix_ctx) => {
            // let desc = create_volume_desc(old_volume, new_volume);
            // prefix_ctx.channel_id().say(pref)ix_ctx.serenity_context.http(), desc).await?;
            prefix_ctx
                .msg
                .reply(
                    old_ctx.http(),
                    embed.0.get("description").unwrap().to_string(),
                )
                .await?;
            Ok(())
        }
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => {
                let mut interaction = x.clone();
                create_embed_response(&old_ctx.http, interaction.borrow_mut(), embed).await?;
                Ok(())
            }
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => {
                panic!("Autocomplete not implemented");
            }
        },
    }
}

pub fn create_volume_embed(old: f32, new: f32) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.description(create_volume_desc(old, new));
    embed
}

pub fn create_volume_desc(old: f32, new: f32) -> String {
    format!("Volume changed from {}% to {}%", old * 100.0, new * 100.0,)
}
