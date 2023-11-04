use crate::{Context, Error};
use ::serenity::builder::{CreateActionRow, CreateButton};
use poise::{serenity_prelude as serenity, CreateReply};

/// Boop the bot!
/// TODO: get this working
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn boop(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id();

    let id_str = format!("{}", uuid_boop);

    ctx.send(
        CreateReply::new()
            .content("I want some boops!")
            .components(vec![CreateActionRow::Buttons(vec![CreateButton::new(
                id_str,
            )
            .style(serenity::ButtonStyle::Primary)
            .label("Boop me!")])]),
    );

    let mut boop_count = 0;
    while let Some(mci) = serenity::ComponentInteraction::new(ctx)
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
            ir.kind(serenity::InteractionType::DeferredUpdateMessage)
        })
        .await?;
    }

    Ok(())
}
