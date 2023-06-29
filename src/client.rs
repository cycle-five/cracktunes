use self::serenity::GatewayIntents;
use crate::handlers::SerenityHandler;
use crate::{Data, Error};
use poise::serenity_prelude as serenity;
use songbird::serenity::SerenityInit;
use std::{env, sync::Arc};

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
        let token = env::var("DISCORD_TOKEN").expect("Fatality! DISCORD_TOKEN not set!");
        let application_id = env::var("DISCORD_APP_ID")
            .expect("Fatality! DISCORD_APP_ID not set!")
            .parse()?;

        let gateway_intents = GatewayIntents::non_privileged();
        let data = Arc::new(Data::default());

        let client = client_builder
            .token(token)
            .intents(gateway_intents)
            .event_handler(SerenityHandler {
                is_loop_running: false.into(),
                data,
            })
            .application_id(application_id)
            .register_songbird();

        Ok(client)
    }

    pub async fn new(token: String) -> Result<Client, Error> {
        let application_id = env::var("DISCORD_APP_ID")
            .expect("Fatality! DISCORD_APP_ID not set!")
            .parse()?;

        let gateway_intents = GatewayIntents::non_privileged();
        let data = Arc::new(Data::default());
        let client = serenity::Client::builder(token, gateway_intents)
            .event_handler(SerenityHandler {
                is_loop_running: false.into(),
                data,
            })
            .application_id(application_id)
            .register_songbird()
            .await?;

        Ok(Client { client })
    }

    pub async fn start(&mut self) -> Result<(), serenity::Error> {
        self.client.start().await
    }
}
