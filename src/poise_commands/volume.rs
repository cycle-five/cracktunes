use self::serenity::builder::CreateEmbed;
use crate::utils::{create_embed_response, create_embed_response_poise};
use crate::{Context, Error};
use ::serenity::http::CacheHttp;
use poise::{serenity_prelude as serenity, ApplicationCommandOrAutocompleteInteraction};
use songbird::tracks::TrackHandle;
use std::borrow::BorrowMut;

#[poise::command(slash_command, prefix_command)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "The volume to set the player to"] level: Option<u32>,
) -> Result<(), Error> {
    tracing::info!("volume");
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            create_embed_response_poise(
                &ctx,
                "I need to be in a voice channel before you can do that.".to_string(),
            )
            .await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();
    let old_ctx = ctx.serenity_context();

    let to_set = match level {
        Some(arg) => Some(arg as isize),
        None => {
            let handler = call.lock().await;
            let track_handle: TrackHandle = handler.queue().current().expect("No track playing");
            let volume = track_handle.get_info().await.unwrap().volume;
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

    let embed = create_volume_embed(old_volume, new_volume);
    match ctx {
        Context::Prefix(prefix_ctx) => {
            prefix_ctx
                .msg
                .reply(
                    old_ctx.http(),
                    embed.0.get("description").unwrap().to_string(),
                )
                .await?;
            // create_embed_response(&Arc::new(*old_ctx.http()), , embed).await?;
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
    embed.description(format!(
        "Volume changed from {}% to {}%",
        old * 100.0,
        new * 100.0
    ));
    embed
}
