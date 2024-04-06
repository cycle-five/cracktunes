use lyric_finder::LyricResult;
use serenity::{all::GuildId, async_trait};

use crate::{
    commands::MyAuxMetadata, errors::CrackedError, interface::create_lyrics_embed, Context, Error,
};

#[async_trait]
pub trait LyricFinderClient {
    async fn get_lyric(&self, query: &str) -> anyhow::Result<lyric_finder::LyricResult>;
}

#[async_trait]
impl LyricFinderClient for lyric_finder::Client {
    async fn get_lyric(&self, query: &str) -> anyhow::Result<lyric_finder::LyricResult> {
        self.get_lyric(query).await
    }
}

/// Search for song lyrics.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn lyrics(
    ctx: Context<'_>,
    #[rest]
    #[description = "The query to search for"]
    query: Option<String>,
) -> Result<(), Error> {
    // The artist field seems to really just get in the way as it's the literal youtube channel name
    // in many cases.
    // let search_artist = track_handle.metadata().artist.clone().unwrap_or_default();
    let query = query_or_title(ctx, query).await?;
    tracing::warn!("searching for lyrics for {}", query);

    let lyric_finder_client = lyric_finder::Client::new();
    let (track, artists, lyric) = do_lyric_query(lyric_finder_client, query).await?;
    create_lyrics_embed(ctx, artists, track, lyric)
        .await
        .map_err(Into::into)
}

pub async fn query_or_title(ctx: Context<'_>, query: Option<String>) -> Result<String, Error> {
    match query {
        Some(query) => Ok(query),
        None => {
            // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
            let guild_id = get_guild_id(ctx).ok_or(CrackedError::NoGuildId)?;
            let manager = songbird::get(ctx.serenity_context()).await.unwrap();
            let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

            let handler = call.lock().await;
            let track_handle = handler
                .queue()
                .current()
                .ok_or(CrackedError::NothingPlaying)?;

            let lock = track_handle.typemap().read().await;
            let MyAuxMetadata::Data(data) = lock.get::<MyAuxMetadata>().unwrap();
            Ok(data
                .track
                .clone()
                .ok_or_else(|| data.title.clone().ok_or(CrackedError::NoTrackName)?))
        },
    }
}

pub async fn do_lyric_query(
    client: impl LyricFinderClient,
    query: String,
) -> Result<(String, String, String), Error> {
    let result = match client.get_lyric(&query).await {
        Ok(result) => Ok::<LyricResult, Error>(result),
        Err(e) => {
            tracing::error!("lyric search failed: {}", e);
            Err(CrackedError::Anyhow(e).into())
        },
    }?;
    let (track, artists, lyric) = match result {
        lyric_finder::LyricResult::Some {
            track,
            artists,
            lyric,
        } => {
            tracing::warn!("{} by {}'s lyric:\n{}", track, artists, lyric);
            (track, artists, lyric)
        },
        lyric_finder::LyricResult::None => {
            tracing::error!("lyric not found! query: {}", query);
            (
                "Unknown".to_string(),
                "Unknown".to_string(),
                "Lyric not found!".to_string(),
            )
        },
    };

    Ok((track, artists, lyric))
}

#[async_trait]
pub trait ContextWithGuildId {
    fn guild_id(&self) -> Option<GuildId>;
}

#[async_trait]
impl<U, E> ContextWithGuildId for poise::Context<'_, U, E> {
    fn guild_id(&self) -> Option<GuildId> {
        Some(GuildId::new(1))
    }
}

fn get_guild_id(ctx: impl ContextWithGuildId) -> Option<GuildId> {
    ctx.guild_id()
}
#[cfg(test)]
mod test {
    use super::*;
    use mockall::predicate::*;
    use mockall::*;

    // Mock your dependencies here
    // For example, a mock `lyric_finder::Client` might look like this
    mock! {
        LyricFinderClient{}

        #[async_trait]
        impl LyricFinderClient for LyricFinderClient {
            async fn get_lyric(&self, query: &str) -> anyhow::Result<lyric_finder::LyricResult>;
        }
    }

    #[tokio::test]
    async fn test_do_lyric_query_not_found() {
        let mut mock_client = MockLyricFinderClient::new();
        mock_client
            .expect_get_lyric()
            .returning(|_| Err(anyhow::Error::msg("Not found")));

        let result = do_lyric_query(mock_client, "Invalid query".to_string()).await;
        assert!(result.is_err());
    }

    // #[tokio::test]
    // async fn test_query_or_title_with_query() {
    //     // Setup the test context and other necessary mock objects
    //     let ctx = ...; // Mocked context
    //     let query = Some("Some query".to_string());

    //     // Perform the test
    //     let result = query_or_title(ctx, query).await;

    //     // Assert the outcome
    //     assert_eq!(result.unwrap(), "Some query");
    // }

    // #[tokio::test]
    // async fn test_query_or_title_without_query() {
    //     // Setup the test context and other necessary mock objects
    //     // let ctx = ...; // Mocked context without a current track
    //     let ctx = poise::ApplicationContext::

    //     // Perform the test
    //     let result = query_or_title(ctx, None).await;

    //     // Assert that an error is returned because there's no current track
    //     assert!(result.is_err());
    // }

    #[tokio::test]
    async fn test_do_lyric_query_found() {
        // Setup the mocked `lyric_finder::Client`
        let mut mock_client = MockLyricFinderClient::new();
        mock_client
            .expect_get_lyric()
            .with(eq("Some query"))
            .times(1)
            .return_once(|_| {
                Ok(lyric_finder::LyricResult::Some {
                    track: "Some track".to_string(),
                    artists: "Some artist".to_string(),
                    lyric: "Some lyrics".to_string(),
                })
            });

        // Perform the test
        let result = do_lyric_query(mock_client, "Some query".to_string()).await;

        // Assert the outcome
        assert_eq!(
            result.unwrap(),
            (
                "Some track".to_string(),
                "Some artist".to_string(),
                "Some lyrics".to_string()
            )
        );
    }
}
