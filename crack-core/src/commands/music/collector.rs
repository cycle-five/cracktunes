use crate::{Context, Error};
use ::serenity::builder::{
    CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditMessage,
};
use poise::{serenity_prelude as serenity, CreateReply};

/// Boop the bot!
/// TODO: get this working
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn boop(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id();

    let id_str = format!("{}", uuid_boop);

    ctx.send(
        CreateReply::default()
            .content("I want some boops!")
            .components(Cow::Owned(vec![CreateActionRow::buttons(Cow::Owned(
                vec![CreateButton::new(id_str)
                    .style(serenity::ButtonStyle::Primary)
                    .label("Boop me!")],
            ))])),
    )
    .await?;

    let mut boop_count = 0;
    while let Some(mci) = serenity::ComponentInteractionCollector::new(ctx.serenity_context().clone().shard)
        .author_id(ctx.author().id)
        .channel_id(ctx.channel_id())
        .timeout(std::time::Duration::from_secs(120))
        .filter(move |mci| mci.data.custom_id == uuid_boop.to_string())
        .await
    {
        boop_count += 1;

        let mut msg = mci.message.clone();
        msg.edit(
            &ctx,
            EditMessage::default().content(format!("Boop count: {}", boop_count)),
        )
        .await?;

        mci.create_response(
            ctx.http(),
            CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::default()),
        )
        .await?;
    }

    Ok(())
}
