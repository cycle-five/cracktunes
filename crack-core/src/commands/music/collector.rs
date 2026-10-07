use crate::messaging::courier;
use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::{Context, Error};
use ::serenity::builder::{
    CreateActionRow, CreateButton, CreateComponent, CreateInteractionResponse,
    CreateInteractionResponseMessage,
};
use poise::serenity_prelude as serenity;

/// Boop the bot!
/// TODO: get this working
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn boop(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id();

    let id_str = format!("{}", uuid_boop);

    let button = vec![CreateComponent::ActionRow(CreateActionRow::buttons(
        Cow::Owned(vec![CreateButton::new(id_str)
            .style(serenity::ButtonStyle::Primary)
            .label("Boop me!")]),
    ))];
    courier::reply_rendered(
        ctx,
        Rendered::text("I want some boops!").with_components(button.clone()),
        false,
    )
    .await?;

    let mut boop_count = 0;
    while let Some(mci) = serenity::ComponentInteractionCollector::new(ctx.serenity_context())
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(std::time::Duration::from_secs(120))
        .filter(move |mci| mci.data.custom_id == uuid_boop.to_string())
        .await
    {
        boop_count += 1;

        // A channel-message edit replaces the components too, so the button
        // rides along or the second boop has nothing to press.
        if let Err(err) = courier::edit_rendered_message(
            &DiscordTransport::of(ctx.serenity_context()),
            mci.message.channel_id,
            mci.message.id,
            Rendered::text(format!("Boop count: {}", boop_count)).with_components(button.clone()),
        )
        .await
        {
            tracing::warn!("boop: could not update the count: {err:?}");
        }

        #[expect(
            clippy::disallowed_methods,
            reason = "component responses move in PR 2"
        )]
        mci.create_response(
            ctx.http(),
            CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::default()),
        )
        .await?;
    }

    Ok(())
}
