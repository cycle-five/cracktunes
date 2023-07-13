use self::serenity::{
    builder::CreateEmbed,
    http::Http,
    model::{
        application::interaction::{
            application_command::ApplicationCommandInteraction, InteractionResponseType,
            MessageInteraction,
        },
        channel::Message,
    },
    Context as SerenityContext, SerenityError,
};
use crate::{
    commands::summon, errors::CrackedError, messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS, Context, Data, Error,
};
use poise::{
    serenity_prelude as serenity, ApplicationCommandOrAutocompleteInteraction, FrameworkError,
    ReplyHandle,
};
use songbird::tracks::TrackHandle;
use std::{sync::Arc, time::Duration};
use url::Url;

pub async fn create_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    create_embed_response_poise(ctx, embed).await
}

pub async fn create_response_poise_text(
    ctx: &Context<'_>,
    message: CrackedMessage,
) -> Result<(), Error> {
    let message_str = format!("{message}");

    create_embed_response_str(ctx, message_str)
        .await
        .map(|_| Ok(()))?
}

pub async fn create_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    message: CrackedMessage,
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

pub async fn edit_response_poise(ctx: Context<'_>, message: CrackedMessage) -> Result<(), Error> {
    let mut embed = CreateEmbed::default();
    embed.description(format!("{message}"));

    match get_interaction(ctx) {
        Some(mut interaction) => {
            let res =
                edit_embed_response(&ctx.serenity_context().http, &mut interaction, embed).await;
            res.map(|_| ())
        }
        None => {
            create_embed_response_poise(ctx, embed).await
            //return Err(Box::new(SerenityError::Other("No interaction found"))),
        }
    }
}

pub async fn edit_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    message: CrackedMessage,
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
) -> Result<Message, Error> {
    ctx.send(|b| b.embed(|e| e.description(message_str)).reply(true))
        .await
        .unwrap()
        .into_message()
        .await
        .map_err(Into::into)
    //.map_err(Into::into)
    // Ok(())
}

pub async fn create_embed_response_poise(
    ctx: Context<'_>,
    embed: CreateEmbed,
) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(mut interaction) => {
            create_embed_response(&ctx.serenity_context().http, &mut interaction, embed).await
        }
        None => create_embed_response_prefix(&ctx, embed)
            .await
            .map(|_| Ok(()))?,
    }
}

pub async fn create_embed_response_prefix(
    ctx: &Context<'_>,
    embed: CreateEmbed,
) -> Result<Message, Error> {
    ctx.send(|builder| {
        builder.embeds.append(&mut vec![embed]);
        builder //.reply(true)
    })
    .await
    .unwrap()
    .into_message()
    .await
    .map_err(Into::into)
}

pub async fn create_embed_response(
    http: &Arc<Http>,
    interaction: &mut ApplicationCommandInteraction,
    embed: CreateEmbed,
) -> Result<(), Error> {
    interaction
        .create_interaction_response(&http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.add_embed(embed.clone()))
        })
        .await
        .map_err(Into::into)
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

pub enum ApplicationCommandOrMessageInteraction {
    ApplicationCommand(ApplicationCommandInteraction),
    Message(MessageInteraction),
}

impl From<MessageInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: MessageInteraction) -> Self {
        Self::Message(message)
    }
}

impl From<ApplicationCommandInteraction> for ApplicationCommandOrMessageInteraction {
    fn from(message: ApplicationCommandInteraction) -> Self {
        Self::ApplicationCommand(message)
    }
}

pub async fn edit_embed_response_poise(ctx: Context<'_>, embed: CreateEmbed) -> Result<(), Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_original_interaction_response(&ctx.serenity_context().http, |message| {
                message.content(" ").add_embed(embed)
            })
            .await
            .map(|_| Ok(()))?,
        None => create_embed_response_poise(ctx, embed).await, //Err(Box::new(SerenityError::Other("No interaction found"))),
    }
}

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let metadata = track.metadata().clone();

    tracing::warn!("metadata: {:?}", metadata);

    embed.author(|author| author.name(CrackedMessage::NowPlaying));
    metadata
        .title
        .as_ref()
        .map(|title| embed.title(title.clone()));

    metadata
        .source_url
        .as_ref()
        .map(|source_url| embed.url(source_url.clone()));

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    embed.field("Progress", format!(">>> {} / {}", position, duration), true);

    match metadata.channel {
        Some(channel) => embed.field("Channel", format!(">>> {}", channel), true),
        None => embed.field("Channel", ">>> N/A", true),
    };

    metadata
        .thumbnail
        .as_ref()
        .map(|thumbnail| embed.thumbnail(thumbnail));

    let source_url = metadata.source_url.unwrap_or_else(|| {
        tracing::warn!("No source url found for track: {:?}", track);
        "".to_string()
    });

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);
    embed.footer(|f| f.text(footer_text).icon_url(footer_icon_url));

    embed
}

pub async fn create_lyrics_embed(
    track_handle: TrackHandle,
    track: String,
    artists: String,
    lyric: String,
) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    let metadata = track_handle.metadata().clone();

    tracing::trace!("lyric: {}", lyric);
    tracing::trace!("track: {}", track);
    tracing::trace!("artists: {}", artists);

    embed.author(|author| author.name(artists));
    embed.title(track.clone());
    embed.description(lyric);

    metadata
        .source_url
        .as_ref()
        .map(|source_url| embed.url(source_url.clone()));

    metadata
        .thumbnail
        .as_ref()
        .map(|thumbnail| embed.thumbnail(thumbnail));

    let source_url = metadata.source_url.unwrap_or_else(|| {
        tracing::warn!("No source url found for track: {:?}", track);
        "".to_string()
    });

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);
    embed.footer(|f| f.text(footer_text).icon_url(footer_icon_url));

    embed
}

pub fn get_footer_info(url: &str) -> (String, String) {
    let url_data = match Url::parse(url) {
        Ok(url_data) => url_data,
        Err(_) => {
            return (
                "Streaming via unknown".to_string(),
                "https://www.google.com/s2/favicons?domain=unknown".to_string(),
            )
        }
    };
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

pub fn check_interaction(result: Result<(), Error>) {
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
        Context::Prefix(_ctx) => None, //Some(ctx.msg.interaction.clone().into()),
    }
}

pub fn get_interaction_new(ctx: Context<'_>) -> Option<ApplicationCommandOrMessageInteraction> {
    match ctx {
        Context::Application(app_ctx) => match app_ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => {
                Some(x.clone().into())
            }
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(_) => None,
        },
        Context::Prefix(ctx) => ctx.msg.interaction.clone().map(|x| x.into()),
    }
}

pub fn get_guild_id(ctx: &Context) -> Option<serenity::GuildId> {
    match ctx {
        Context::Application(ctx) => ctx.interaction.guild_id(),
        Context::Prefix(ctx) => ctx.msg.guild_id,
    }
}

pub fn get_user_id(ctx: &Context) -> serenity::UserId {
    match ctx {
        Context::Application(ctx) => match ctx.interaction {
            ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(x) => x.user.id,
            ApplicationCommandOrAutocompleteInteraction::Autocomplete(x) => x.user.id,
        },
        Context::Prefix(ctx) => ctx.msg.author.id,
    }
}

pub fn get_channel_id(ctx: &Context) -> serenity::ChannelId {
    match ctx {
        Context::Application(ctx) => ctx.interaction.channel_id(),
        Context::Prefix(ctx) => ctx.msg.channel_id,
    }
}

pub async fn summon_short(ctx: Context<'_>) -> Result<(), FrameworkError<Data, Error>> {
    match ctx {
        Context::Application(prefix_ctx) => {
            let guild_id = prefix_ctx.guild_id().unwrap();
            let manager = songbird::get(prefix_ctx.serenity_context()).await.unwrap();
            let call = manager.get(guild_id).unwrap();
            let handler = call.lock().await;
            let has_current_connection = handler.current_connection().is_some();
            drop(handler);

            if !has_current_connection {
                summon().slash_action.unwrap()(prefix_ctx).await
            } else {
                Ok(())
            }
        }
        Context::Prefix(slash_ctx) => {
            let guild_id = slash_ctx.guild_id().unwrap();
            let manager = songbird::get(slash_ctx.serenity_context()).await.unwrap();
            let call = manager.get(guild_id).unwrap();
            let handler = call.lock().await;
            let has_current_connection = handler.current_connection().is_some();
            drop(handler);

            if !has_current_connection {
                summon().prefix_action.unwrap()(slash_ctx).await
            } else {
                Ok(())
            }
        }
    }
}
pub async fn handle_error(
    ctx: &SerenityContext,
    interaction: &mut ApplicationCommandInteraction,
    err: CrackedError,
) {
    create_response_text(&ctx.http, interaction, &format!("{err}"))
        .await
        .expect("failed to create response");
}
pub fn count_command(command: &str, is_prefix: bool) {
    tracing::warn!("counting command: {}", command);
    match COMMAND_EXECUTIONS
        .get_metric_with_label_values(&[command, if is_prefix { "prefix" } else { "slash" }])
    {
        Ok(metric) => {
            metric.inc();
        }
        Err(e) => {
            tracing::error!("Failed to get metric: {}", e);
        }
    };
}
