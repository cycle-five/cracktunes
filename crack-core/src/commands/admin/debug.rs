use crack_types::CrackedError;
use crate::guild::settings::GuildSettingsMap;
use crate::utils::send_embed_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::CreateEmbed;
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
    let call = match manager.get(guild.id) {
        Some(call) => call,
        None => {
            let embed =
                CreateEmbed::default().description(format!("{}", CrackedError::NotConnected));
            send_embed_response_poise(ctx, embed).await?;
            return Ok(());
        },
    };
    let handler = call.lock().await;
    let queue = handler.queue();

    let queue_str = queue_to_str(queue);
    // let global_handlers = get_global_handlers(ctx);

    let embed = CreateEmbed::default().description(format!(
        "data: {}old_data_str{}\nqueue: {}",
        data_str, old_data_str, queue_str
    ));
    send_embed_response_poise(ctx, embed).await?;

    Ok(())
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
