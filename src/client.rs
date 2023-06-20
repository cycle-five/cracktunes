use crate::{Data, Error};
use poise::serenity_prelude as serenity;
use songbird::serenity::SerenityInit;

//use self::serenity::model::gateway::GatewayIntents;
use self::serenity::GatewayIntents;
use std::{collections::HashMap, env};

use crate::{
    guild::{cache::GuildCacheMap, settings::GuildSettingsMap},
    handlers::SerenityHandler,
};

pub struct Client {
    client: serenity::Client,
}

impl Client {
    pub async fn default() -> Result<Client, Error> {
        let token = env::var("DISCORD_TOKEN").expect("Fatality! DISCORD_TOKEN not set!");
        Client::new(token).await
    }

    pub async fn client_builder(
        client_builder: serenity::ClientBuilder,
    ) -> Result<serenity::ClientBuilder, Error> {
        //) -> serenity::ClientBuilder + Send + Sync + 'static {
        let token = env::var("DISCORD_TOKEN").expect("Fatality! DISCORD_TOKEN not set!");
        let application_id = env::var("DISCORD_APP_ID")
            .expect("Fatality! DISCORD_APP_ID not set!")
            .parse()?;

        let gateway_intents = GatewayIntents::non_privileged();
        let data = Data {
            ..Default::default()
        };
        // let data = Arc::new(data);

        let client = //serenity::Client::builder(token, gateway_intents)
            client_builder
            .token(token)
            .intents(gateway_intents)
            .event_handler(SerenityHandler {is_loop_running: false.into(), data})
            .application_id(application_id)
            .register_songbird();

        Ok(client)
    }

    pub async fn new(token: String) -> Result<Client, Error> {
        let application_id = env::var("DISCORD_APP_ID")
            .expect("Fatality! DISCORD_APP_ID not set!")
            .parse()?;

        let gateway_intents = GatewayIntents::non_privileged();
        let data = Data {
            ..Default::default()
        };
        let client = serenity::Client::builder(token, gateway_intents)
            .event_handler(SerenityHandler {
                is_loop_running: false.into(),
                data,
            })
            .application_id(application_id)
            .register_songbird()
            .await?;

        let mut data = client.data.write().await;
        data.insert::<GuildCacheMap>(HashMap::default());
        data.insert::<GuildSettingsMap>(HashMap::default());
        drop(data);

        Ok(Client { client })
    }

    pub async fn start(&mut self) -> Result<(), serenity::Error> {
        self.client.start().await
    }
}
