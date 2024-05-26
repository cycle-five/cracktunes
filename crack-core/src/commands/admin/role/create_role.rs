use poise::serenity_prelude::{Colour, Permissions};
use serenity::all::{Attachment, CreateAttachment, GuildId, Role};
use serenity::builder::EditRole;

use crate::commands::{ConvertToEmptyResult, EmptyResult};
use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;

/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Name of the role to create."] name: String,
    #[description = "Whether the role is hoisted."] hoist: Option<bool>,
    #[description = "Whether the role is mentionable."] mentionable: Option<bool>,
    #[description = "Optional initial perms"] permissions: Option<u64>,
    #[description = "Optional initial position (vertical)"] position: Option<u16>,
    #[description = "Optional initial colour"] colour: Option<u32>,
    #[description = "Optional emoji"] unicode_emoji: Option<String>,
    #[description = "Optional reason for the audit_log"] audit_log_reason: Option<String>,
    #[description = "Optional initial perms"] icon: Option<Attachment>,
) -> EmptyResult {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let icon = match icon {
        Some(attachment) => {
            let url = attachment.url.clone();
            Some(CreateAttachment::url(ctx, &url).await?)
        },
        None => None,
    };

    let role = create_role_internal(
        ctx,
        guild_id,
        name,
        hoist,
        mentionable,
        permissions,
        position,
        colour,
        unicode_emoji,
        audit_log_reason,
        icon.as_ref(),
    )
    .await?;

    send_response_poise(
        ctx,
        CrackedMessage::RoleCreated {
            role_name: role.name.clone(),
            role_id: role.id,
        },
        true,
    )
    .await
    .convert()
}

/// Internal create role function.
pub async fn create_role_internal(
    ctx: Context<'_>,
    guild_id: GuildId,
    name: String,
    hoist: Option<bool>,
    mentionable: Option<bool>,
    permissions: Option<u64>,
    position: Option<u16>,
    colour: Option<u32>,
    unicode_emoji: Option<String>,
    audit_log_reason: Option<String>,
    icon: Option<&CreateAttachment>,
) -> Result<Role, CrackedError> {
    let perms = Permissions::from_bits(permissions.unwrap_or_default())
        .ok_or(CrackedError::InvalidPermissions)?;
    let colour = colour.map(Colour::new).unwrap_or_default();
    let audit_log_reason = audit_log_reason.unwrap_or_default();
    let role_builder = EditRole::default()
        .name(name)
        .hoist(hoist.unwrap_or_default())
        .mentionable(mentionable.unwrap_or_default())
        .permissions(Into::into(perms))
        .position(position.unwrap_or_default())
        .colour(colour)
        .unicode_emoji(unicode_emoji)
        .audit_log_reason(&audit_log_reason)
        .icon(icon);
    guild_id
        .create_role(&ctx, role_builder)
        .await
        .map_err(Into::into)
}
