use self::serenity::{
    builder::CreateEmbed,
    http::{Http, HttpError},
    model::{
        application::interaction::{
            application_command::ApplicationCommandInteraction, InteractionResponseType,
        },
        channel::Message,
    },
};
//use ::serenity::http::CacheHttp;
use poise::{
    serenity_prelude as serenity, ApplicationCommandOrAutocompleteInteraction, ReplyHandle,
};
use songbird::tracks::TrackHandle;
use std::{sync::Arc, time::Duration};
use url::Url;

use crate::{messaging::message::ParrotMessage, Context, Error};
use poise::serenity_prelude::SerenityError;

pub async fn create_response_poise(ctx: Context<'_>, message: ParrotMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    create_embed_response_poise(ctx, embed).await
}

pub async fn create_response_poise_text(
    ctx: &Context<'_>,
    message: ParrotMessage,
) -> Result<(), Error> {
    let message_str = format!("{message}");

    create_embed_response_str(&ctx, message_str).await
}

pub async fn create_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    message: ParrotMessage,
) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));
    create_embed_response(http, interaction, embed).await
}

pub async fn create_response_text(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    content: &str,
) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(content);
    create_embed_response(http, interaction, embed).await
}

pub async fn edit_response_poise(ctx: Context<'_>, message: ParrotMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    match get_interaction(ctx) {
        Some(mut interaction) => {
            let res =
                edit_embed_response(&ctx.serenity_context().http, &mut interaction, embed).await;
            return res.map(|_| ());
        }
        None => {
            return create_embed_response_poise(ctx, embed).await;
            //return Err(Box::new(SerenityError::Other("No interaction found"))),
        }
    }
}

pub async fn edit_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    message: ParrotMessage,
) -> Result<Message, Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));
    edit_embed_response(http, interaction, embed).await
}

pub async fn edit_response_text(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    content: &str,
) -> Result<Message, Error> {
    let mut embed = CreateEmbed::default();
    embed.description(content);
    edit_embed_response(http, interaction, embed).await
}

pub async fn create_embed_response_str(
    ctx: &Context<'_>,
    message_str: String,
) -> Result<(), Error> {
    ctx.send(|b| b.embed(|e| e.description(message_str)).reply(true))
        .await?;
    Ok(())
}

pub async fn create_embed_response_poise(
    ctx: Context<'_>,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(mut interaction) => {
            return create_embed_response(&ctx.serenity_context().http, &mut interaction, embed)
                .await;
        }
        None => {
            //ctx.defer().await?;
            //let mut interaction = get_interaction(ctx).unwrap();
            let asdf = format!("{:?}", embed);
            return create_embed_response_str(&ctx, asdf).await;
        }
    }
}

pub async fn create_embed_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match interaction
        .create_interaction_response(&http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.add_embed(embed.clone()))
        })
        .await
        .map_err(Into::into)
    {
        Ok(val) => Ok(val),
        Err(err) => match err {
            serenity::Error::Http(ref e) => match &**e {
                HttpError::UnsuccessfulRequest(req) => match req.error.code {
                    40060 => edit_embed_response(http, interaction, embed)
                        .await
                        .map(|_| ()),
                    _ => Err(Box::new(err)),
                },
                _ => Err(Box::new(err)),
            },
            _ => Err(Box::new(err)),
        },
    }
}

pub async fn edit_embed_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    interaction
        .edit_original_interaction_response(&http, |message| message.content(" ").add_embed(embed))
        .await
        .map_err(Into::into)
}

pub async fn edit_embed_response_poise(
    ctx: Context<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_original_interaction_response(&ctx.serenity_context().http, |message| {
                message.content(" ").add_embed(embed)
            })
            .await
            .map_err(Into::into),
        None => return Err(Box::new(SerenityError::Other("No interaction found"))),
    }
}

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let metadata = track.metadata().clone();

    embed.author(|author| author.name(ParrotMessage::NowPlaying));
    embed.title(metadata.title.unwrap());
    embed.url(metadata.source_url.as_ref().unwrap());

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    embed.field("Progress", format!(">>> {} / {}", position, duration), true);

    match metadata.channel {
        Some(channel) => embed.field("Channel", format!(">>> {}", channel), true),
        None => embed.field("Channel", ">>> N/A", true),
    };

    embed.thumbnail(&metadata.thumbnail.unwrap());

    let source_url = metadata.source_url.as_ref().unwrap();

    let (footer_text, footer_icon_url) = get_footer_info(source_url);
    embed.footer(|f| f.text(footer_text).icon_url(footer_icon_url));

    embed
}

pub fn get_footer_info(url: &str) -> (String, String) {
    let url_data = Url::parse(url).unwrap();
    let domain = url_data.host_str().unwrap();

    // remove www prefix because it looks ugly
    let domain = domain.replace("www.", "");

    (
        format!("Streaming via {}", domain),
        format!("https://www.google.com/s2/favicons?domain={}", domain),
    )
}

pub fn get_human_readable_timestamp(duration: Option<Duration>) -> String {
    match duration {
        Some(duration) if duration == Duration::MAX => "∞".to_string(),
        Some(duration) => {
            let seconds = duration.as_secs() % 60;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = duration.as_secs() / 3600;

            if hours < 1 {
                format!("{:02}:{:02}", minutes, seconds)
            } else {
                format!("{}:{:02}:{:02}", hours, minutes, seconds)
            }
        }
        None => "∞".to_string(),
    }
}

pub fn compare_domains(domain: &str, subdomain: &str) -> bool {
    subdomain == domain || subdomain.ends_with(domain)
}

/// Checks that a message successfully sent; if not, then logs why to stdout.
pub fn check_msg(result: Result<Message, Error>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub fn check_reply(result: Result<ReplyHandle, SerenityError>) {
    if let Err(why) = result {
        tracing::error!("Error sending message: {:?}", why);
    }
}

pub fn get_interaction(ctx: Context<'_>) -> Option<ApplicationCommandInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => Some(x.clone()),
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(_) => None,
    }
}

pub fn get_guild_id(ctx: &Context) -> Option<serenity::GuildId> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => x.guild_id,
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.guild_id,
        },
        Context::Prefix(pre_ctx) => pre_ctx.msg.guild_id,
    }
}

pub fn get_user_id(ctx: &Context) -> serenity::UserId {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => x.user.id,
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.user.id,
        },
        Context::Prefix(pre_ctx) => pre_ctx.msg.author.id,
    }
}

pub fn get_channel_id(ctx: &Context) -> serenity::ChannelId {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => x.channel_id,
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.channel_id,
        },
        Context::Prefix(pre_ctx) => pre_ctx.msg.channel_id,
    }
}

// pub fn reply_poise(ctx: &Context, content: String) -> Result<Message, Error> {
//     //ctx.reply(content)
//     match ctx {
//         Context::Application(app_ctx) => match app_ctx.interaction {
//             ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.channel_id,
//         },
//         Context::Prefix(pre_ctx) => pre_ctx.msg.reply_mention(ctx.http(), content),
//     }
// }
