use crate::{
    commands::MyAuxMetadata, errors::CrackedError, http_utils,
    messaging::interface::create_lyrics_embed, Context, poise_ext::ContextExt, Error,
};

#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
/// Search for song lyrics.
pub async fn lyrics(
    ctx: Context<'_>,
    #[rest]
    #[description = "The query to search for"]
    query: Option<String>,
) -> Result<(), Error> {
    let query = query_or_title(ctx, query).await?;
    tracing::warn!("searching for lyrics for {}", query);

    let client = lyric_finder::Client::from_http_client(http_utils::get_client());

    let res = client.get_lyric(&query).await?;
    create_lyrics_embed(ctx, res).await.map_err(Into::into)
}

#[cfg(not(tarpaulin_include))]
/// Internal function for searching for song lyrics.
pub async fn lyrics_internal(ctx: Context<'_>, query: Option<String>) -> Result<(), Error> {
    let query = query_or_title(ctx, query).await?;
    tracing::warn!("searching for lyrics for {}", query);

    let client = lyric_finder::Client::from_http_client(http_utils::get_client());

    let res = client.get_lyric(&query).await?;
    create_lyrics_embed(ctx, res).await.map_err(Into::into)
}

/// Get the current track name as either the query or the title of the current track.
#[cfg(not(tarpaulin_include))]
pub async fn query_or_title(ctx: Context<'_>, query: Option<String>) -> Result<String, Error> {
    match query {
        Some(query) => Ok(query),
        None => {
            let call = ctx.get_call().await?;
            let handler = call.lock().await;
            let track_handle = handler
                .queue()
                .current()
                .ok_or(CrackedError::NothingPlaying)?;

            let lock = track_handle.typemap().read().await;
            let MyAuxMetadata::Data(data) = lock.get::<MyAuxMetadata>().unwrap();
            tracing::info!("data: {:?}", data);
            data.track
                .clone()
                .or(data.title.clone())
                .ok_or(CrackedError::NoTrackName.into())
        },
    }
}

// pub async fn do_lyric_query(
//     client: lyric_finder::Client,
//     query: String,
// ) -> Result<(String, String, String), Error> {
//     let (track, artists, lyric) = match client.get_lyric(&query).await {
//         Ok(lyric_finder::LyricResult::Some {
//             track,
//             artists,
//             lyric,
//         }) => {
//             tracing::warn!("{} by {}'s lyric:\n{}", track, artists, lyric);
//             (track, artists, lyric)
//         },
//         Ok(lyric_finder::LyricResult::None) => {
//             tracing::error!("lyric not found! query: {}", query);
//             (
//                 UNKNOWN.to_string(),
//                 UNKNOWN.to_string(),
//                 "Lyric not found!".to_string(),
//             )
//         },
//         Err(e) => {
//             tracing::error!("lyric query error: {}", e);
//             return Err(e.into());
//         },
//     };

//     Ok((track, artists, lyric))
// }

// #[cfg(test)]
// mod test {
//     use super::*;

//     // // Mock your dependencies here
//     // // For example, a mock `lyric_finder::Client` might look like this
//     // mock! {
//     //     LyricFinderClient{}

//     //     #[async_trait]
//     //     impl LyricFinderClient for LyricFinderClient {
//     //         async fn get_lyric(&self, query: &str) -> anyhow::Result<lyric_finder::LyricResult>;
//     //     }
//     // }

//     #[tokio::test]
//     async fn test_do_lyric_query_not_found() {
//         let client = lyric_finder::Client::new();
//         let result = do_lyric_query(client, "Hit That The Offpspring".to_string()).await;
//         match result {
//             Ok((track, artists, lyric)) => {
//                 assert_eq!(track, "Hit That");
//                 assert_eq!(artists, "The Offspring");
//                 assert_ne!(lyric, "Lyric not found!");
//             },
//             Err(_) => panic!("Unexpected error"),
//         }
//     }

//     // #[tokio::test]
//     // async fn test_query_or_title_with_query() {
//     //     // Setup the test context and other necessary mock objects
//     //     let ctx = ...; // Mocked context
//     //     let query = Some("Some query".to_string());

//     //     // Perform the test
//     //     let result = query_or_title(&ctx, query).await;

//     //     // Assert the outcome
//     //     assert_eq!(result.unwrap(), "Some query");
//     // }

//     // #[tokio::test]
//     // async fn test_query_or_title_without_query() {
//     //     // Setup the test context and other necessary mock objects
//     //     // let ctx = ...; // Mocked context without a current track
//     //     let ctx = poise::ApplicationContext::

//     //     // Perform the test
//     //     let result = query_or_title(&ctx, None).await;

//     //     // Assert that an error is returned because there's no current track
//     //     assert!(result.is_err());
//     // }

//     // #[tokio::test]
//     // async fn test_do_lyric_query_found() {
//     //     // Setup the mocked `lyric_finder::Client`
//     //     let mut mock_client = MockLyricFinderClient::new();
//     //     mock_client
//     //         .expect_get_lyric()
//     //         .with(eq("Some query"))
//     //         .times(1)
//     //         .return_once(|_| {
//     //             Ok(lyric_finder::LyricResult::Some {
//     //                 track: "Some track".to_string(),
//     //                 artists: "Some artist".to_string(),
//     //                 lyric: "Some lyrics".to_string(),
//     //             })
//     //         });

//     //     // Perform the test
//     //     let result = do_lyric_query(mock_client, "Some query".to_string()).await;

//     //     // Assert the outcome
//     //     assert_eq!(
//     //         result.unwrap(),
//     //         (
//     //             "Some track".to_string(),
//     //             "Some artist".to_string(),
//     //             "Some lyrics".to_string()
//     //         )
//     //     );
//     // }
// }
