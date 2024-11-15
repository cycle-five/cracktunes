use crate::commands::help;
use crate::errors::CrackedError;
use crate::CrackedResult;
use crate::{Context, Error};
use serenity::all::{ChannelId, GuildId};

/// Struct that defines a smoke test to run.
#[derive(Debug, Clone)]
pub struct SmokeTest<'a> {
    ctx: Context<'a>,
    chan: ChannelId,
    say_msg: String,
    wait_secs: Option<u64>,
    want_response: Option<String>,
}

/// Implemention of the SmokeTest struct.
impl<'a> SmokeTest<'a> {
    pub fn new(ctx: Context<'a>, chan: ChannelId, say_msg: String) -> Self {
        Self {
            ctx,
            chan,
            say_msg,
            wait_secs: Some(2),
            want_response: None,
        }
    }

    pub fn new_generator(ctx: Context<'a>, chan: ChannelId) -> impl Fn(String) -> Self {
        move |say_msg| SmokeTest {
            ctx,
            chan,
            say_msg,
            wait_secs: Some(2),
            want_response: None,
        }
    }

    pub fn with_wait_secs(mut self, wait_secs: u64) -> Self {
        self.wait_secs = Some(wait_secs);
        self
    }

    pub fn with_want_response(mut self, want_response: String) -> Self {
        self.want_response = Some(want_response);
        self
    }

    pub async fn run(&self) -> CrackedResult<()> {
        run_smoke_test(self.clone()).await
    }
}

/// Have the bot say something in a channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Testing",
    slash_command,
    prefix_command,
    owners_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn smoketest(
    ctx: Context<'_>,
    #[flag]
    #[description = "show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }

    smoketest_internal(ctx).await
}

/// Run the smoke tests.
pub async fn smoketest_internal(ctx: Context<'_>) -> Result<(), Error> {
    let beg = std::time::SystemTime::now();

    let test_chan = ChannelId::new(1232025110802862180);
    let _test_guild = GuildId::new(1220832110210846800);

    // Send message to testing channel to trigger the testee bot to respond
    let tests = get_all_test_messages();
    let test_gen = SmokeTest::new_generator(ctx, test_chan);
    for test_msg in tests {
        let test = test_gen(test_msg);
        test.run().await?;
    }

    let end = std::time::SystemTime::now();
    let delta = end.duration_since(beg)?.as_secs();

    tracing::info!("Smoke test took {} seconds", delta);

    Ok(())
}

/// Get all the test messages to send for the smoke tests.
pub fn get_all_test_messages() -> Vec<String> {
    vec![
        "Beginning Some Test...",
        "{test}!invite",
        "{test}!ping",
        "{test}!version",
        "{test}!servers",
        "{test}!uptime",
        "{test}!clean",
        "{test}!vote",
        // Settings
        "{test}!settings",
        "{test}!settings get",
        "{test}!settings get auto_role",
        "{test}!settings get idle_timeouit",
        "{test}!settings get premium",
        "{test}!settings get volume",
        "{test}!settings get welcome_settings",
        "{test}!settings get log_channels",
        "{test}!say_channel <#1232025110802862180> Smoke Test...",
        "{test}!say_channel_id 1232025110802862180 Complete.",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

/// Run a smoke test.
pub async fn run_smoke_test(test: SmokeTest<'_>) -> CrackedResult<()> {
    match test.chan.say(&test.ctx.http(), test.say_msg).await {
        Ok(_) => (),
        Err(e) => {
            tracing::error!("Error sending message: {e:?}");
            return Err(CrackedError::Other("Error sending message".to_string()));
        },
    }
    if let Some(wait_secs) = test.wait_secs {
        tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
    }
    if let Some(want_response) = &test.want_response {
        // let response = test.ctx.await?;
        tracing::info!("Want response: {:?}", want_response);
    }
    Ok(())
}
