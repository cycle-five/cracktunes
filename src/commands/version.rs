use self::serenity::{
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{messaging::message::ParrotMessage, utils::create_response, Error};
use poise::serenity_prelude as serenity;

pub async fn version(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), Error> {
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    create_response(
        &ctx.http,
        interaction,
        ParrotMessage::Version {
            current: current.to_owned(),
        },
    )
    .await
}
