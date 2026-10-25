use crate::messaging::render::Rendered;
use crate::messaging::transport::DiscordTransport;
use crate::messaging::{courier, discord};
use crate::{Context, Error};
use ::serenity::builder::{CreateInteractionResponse, CreateInteractionResponseMessage};
use crack_model::{Button, ButtonStyle, Row};
use poise::serenity_prelude as serenity;

/// The boop button: its custom id is the command's invocation id.
fn boop_button(id: String) -> Vec<Row> {
    vec![Row::Buttons(vec![Button {
        id,
        label: Some("Boop me!".to_owned()),
        emoji: None,
        style: ButtonStyle::Primary,
        disabled: false,
    }])]
}

/// Boop the bot!
/// TODO: get this working
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn boop(ctx: Context<'_>) -> Result<(), Error> {
    let uuid_boop = ctx.id();

    let id_str = format!("{}", uuid_boop);

    let button = discord::action_rows(&boop_button(id_str));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boop_button_renders_as_before() {
        assert_eq!(
            serde_json::to_string(&discord::action_rows(&boop_button("42".to_string()))).unwrap(),
            r#"[{"type":1,"components":[{"style":1,"type":2,"custom_id":"42","label":"Boop me!","disabled":false}]}]"#
        );
    }
}
