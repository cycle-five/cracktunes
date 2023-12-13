use self::serenity::{
    builder::CreateInputText,
    collector::ModalInteractionCollector,
    futures::StreamExt,
    model::prelude::{ActionRowComponent, InputTextStyle},
};
use crate::{
    guild::settings::{GuildSettings, GuildSettingsMap},
    messaging::messages::{
        DOMAIN_FORM_ALLOWED_PLACEHOLDER, DOMAIN_FORM_ALLOWED_TITLE, DOMAIN_FORM_BANNED_PLACEHOLDER,
        DOMAIN_FORM_BANNED_TITLE, DOMAIN_FORM_TITLE,
    },
    utils::get_interaction,
    Context, Error,
};
use ::serenity::builder::CreateActionRow;
use poise::serenity_prelude as serenity;

/// Manage the domains that are allowed or banned.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn allow(ctx: Context<'_>) -> Result<(), Error> {
    let prefix = ctx.data().bot_settings.get_prefix();
    let interaction = get_interaction(ctx).unwrap();
    let guild_id = interaction.guild_id.unwrap();

    let mut data = ctx.serenity_context().data.write().await;
    let settings = data.get_mut::<GuildSettingsMap>().unwrap();

    use ::serenity::builder::{CreateInteractionResponse, CreateModal};

    use crate::utils::get_guild_name;
    let guild_settings = settings.entry(guild_id).or_insert_with(|| {
        GuildSettings::new(
            guild_id,
            Some(&prefix),
            get_guild_name(ctx.serenity_context(), guild_id),
        )
    });

    // transform the domain sets from the settings into a string
    let allowed_str = guild_settings
        .allowed_domains
        .clone()
        .into_iter()
        .collect::<Vec<String>>()
        .join(";");

    let banned_str = guild_settings
        .banned_domains
        .clone()
        .into_iter()
        .collect::<Vec<String>>()
        .join(";");

    drop(data);

    let allowed_input = CreateInputText::new(
        InputTextStyle::Paragraph,
        DOMAIN_FORM_ALLOWED_TITLE,
        "allowed_domains",
    )
    .placeholder(DOMAIN_FORM_ALLOWED_PLACEHOLDER)
    .value(allowed_str)
    .required(false);

    let banned_input = CreateInputText::new(
        InputTextStyle::Paragraph,
        DOMAIN_FORM_BANNED_TITLE,
        "banned_domains",
    )
    .placeholder(DOMAIN_FORM_BANNED_PLACEHOLDER)
    .value(banned_str)
    .required(false);

    let components = vec![
        CreateActionRow::InputText(allowed_input.clone()),
        CreateActionRow::InputText(banned_input.clone()),
    ];

    let interaction_response = CreateInteractionResponse::Modal(
        CreateModal::new("manage_domains", DOMAIN_FORM_TITLE).components(components.clone()),
    );
    interaction
        .create_response(&ctx.serenity_context().http, interaction_response)
        .await?;

    // collect the submitted data
    let collector = ModalInteractionCollector::new(ctx)
        .filter(|int| int.data.custom_id == "manage_domains")
        .stream();

    collector
        .then(|int| async move {
            let mut data = ctx.serenity_context().data.write().await;
            let settings = data.get_mut::<GuildSettingsMap>().unwrap();

            let inputs: Vec<_> = int
                .data
                .components
                .iter()
                .flat_map(|r| r.components.iter())
                .collect();

            let guild_settings = settings.get_mut(&guild_id).unwrap();

            for input in inputs.iter() {
                if let ActionRowComponent::InputText(it) = input {
                    if it.custom_id == "allowed_domains" {
                        guild_settings.set_allowed_domains(&it.value.clone().unwrap_or_default());
                    }

                    if it.custom_id == "banned_domains" {
                        guild_settings.set_banned_domains(&it.value.clone().unwrap_or_default());
                    }
                }
            }

            guild_settings.update_domains();
            guild_settings.save().await.unwrap();

            // it's now safe to close the modal, so send a response to it
            int.create_response(
                &ctx.serenity_context().http,
                CreateInteractionResponse::Modal(CreateModal::new(
                    "manage_domains",
                    DOMAIN_FORM_TITLE,
                )),
            )
            .await
            .ok();
        })
        .collect::<Vec<_>>()
        .await;

    Ok(())
}
