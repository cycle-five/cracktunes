use crate::Context;
use crate::Error;
use serenity::all::Guild;
use std::collections::BTreeMap;
use std::fmt::Write;

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn track_invites(
    ctx: Context<'_>,
    #[description = "Guild to track invites for."] guild: Guild,
) -> Result<(), Error> {
    // Get all the invites for the guild
    let invites = guild.invites(&ctx).await?;
    let _inviter_by_code = invites
        .iter()
        .map(|invite| (invite.code.clone(), invite.inviter.clone()))
        .collect::<BTreeMap<_, _>>();

    let invites_by_user = invites
        .iter()
        .map(|invite| {
            (
                invite.inviter.as_ref().map(|x| x.id).unwrap_or_default(),
                invite,
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut invites_by_user_string = "{ ".to_string();
    invites_by_user.iter().for_each(|(key, &value)| {
        let _ = write!(
            &mut invites_by_user_string,
            "{}: {} {}/{}, \n",
            key, value.code, value.uses, value.max_uses
        );
    });
    invites_by_user_string.pop(); // Remove the last comma.
    invites_by_user_string.pop(); // Remove the last space.
    invites_by_user_string.push_str(" }");
    tracing::warn!("invites_by_user: {}", invites_by_user_string);

    ctx.say(invites_by_user_string).await?;

    Ok(())
}

fn _kv_iter_to_string(
    iter: impl Iterator<Item = (impl std::fmt::Display, impl std::fmt::Display)>,
) -> String {
    let mut string = "{ ".to_string();
    iter.for_each(|(key, value)| {
        let _ = write!(&mut string, "{}: {}, \n", key, value);
    });
    string.pop(); // Remove the last comma.
    string.pop(); // Remove the last space.
    string.push_str(" }");
    string
}
