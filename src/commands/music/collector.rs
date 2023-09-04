use crate::{Context, Error};
use ::serenity::{
    all::{ActivityButton, ComponentInteraction},
    builder::{CreateActionRow, CreateButton},
    collector::ComponentInteractionCollector,
};
use poise::{serenity_prelude as serenity, CreateReply};

/// Boop the bot!
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn boop(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id().to_string();

    ctx.send(
        CreateReply::default()
            .content("I want some boops!")
            .components(vec![CreateActionRow::Buttons(vec![CreateButton::new(
                "boop",
            )
            .style(serenity::ButtonStyle::Primary)
            .label("Boop me!")
            .custom_id(uuid_boop)])]),
    )
    .await?;
    // .push(ActivityButton::new(
    //     ActivityButton::Style::Primary,
    //     "Boop me!",
    //     uuid_boop,
    // ))

    let mut boop_count = 0;
    while let Some(mci) = serenity::ComponentInteraction::from(value)
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(std::time::Duration::from_secs(120))
        .filter(move |mci| mci.data.custom_id == uuid_boop.to_string())
        .await
    {
        boop_count += 1;

        let mut msg = mci.message.clone();
        msg.edit(ctx, |m| m.content(format!("Boop count: {}", boop_count)))
            .await?;

        mci.create_interaction_response(ctx, |ir| {
            ir.kind(serenity::InteractionResponseFlags::DeferredUpdateMessage)
        })
        .await?;
    }

    Ok(())
}
