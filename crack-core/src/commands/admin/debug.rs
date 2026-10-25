use crate::errors::CrackedError;
use crate::guild::settings::GuildSettingsMap;
use crate::utils::send_embed_response_poise;
use crate::Context;
use crate::Error;
use crack_model::Card;
use songbird::tracks::TrackQueue;

/// Print some debug info.
#[poise::command(category = "Admin", prefix_command, owners_only, ephemeral)]
pub async fn debugold(ctx: Context<'_>) -> Result<(), Error> {
    let data = ctx.data();

    let data_str = format!("{:#?}", data);

    let mut old_data_str = String::new();
    // let guild_settings_map = lock.get::<GuildSettingsMap>().unwrap();
    let guild_settings_map = data.guild_settings_map.write().await;
    guild_settings_map.iter().for_each(|(k, v)| {
        old_data_str.push_str(&format!("k: {:?}, v: {:?}", k, v));
    });

    let guild_id = ctx.guild_id().unwrap();
    let guild = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .unwrap()
        .clone();
    let manager = data.songbird.clone();
    // Raw `get` on purpose (#507): an admin queue dump wants whatever songbird
    // holds. A Call stranded by a failed join is exactly what someone debugging
    // needs to see, not have filtered out.
    #[allow(clippy::disallowed_methods)]
    let call = match manager.get(guild.id) {
        Some(call) => call,
        None => {
            let card = note_card(format!("{}", CrackedError::NotConnected));
            send_embed_response_poise(ctx, card).await?;
            return Ok(());
        },
    };
    let handler = call.lock().await;
    let queue = handler.queue();

    let queue_str = queue_to_str(queue);
    // let global_handlers = get_global_handlers(ctx);

    let card = note_card(format!(
        "data: {}old_data_str{}\nqueue: {}",
        data_str, old_data_str, queue_str
    ));
    send_embed_response_poise(ctx, card).await?;

    Ok(())
}

/// A card that is only its description.
fn note_card(description: String) -> Card {
    Card {
        description: Some(description),
        ..Card::default()
    }
}

/// Simple helper to convert a `[TrackQueue]' to a `[String]`.
pub fn queue_to_str(queue: &TrackQueue) -> String {
    let tracks = queue.current_queue();
    let mut buf = String::new();
    for track in tracks {
        buf.push_str(&format!("track: {:?}", track));
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::discord;

    #[test]
    fn the_debug_note_renders_as_before() {
        let wire = serde_json::to_string(&discord::embed(&note_card("hello there".into())));
        assert_eq!(
            wire.unwrap(),
            r#"{"type":"rich","description":"hello there"}"#
        );
    }
}
