//! The play log's writer: one background task that records each queued track
//! (its metadata, the member, and a `play_log` row) off the command's path.
//!
//! 🔑 One task, not a pool. Senders `try_send`, so /play never waits on these
//! writes, and a 300-track playlist's writes go through one connection, one at
//! a time, instead of taking every connection in the bot's pool.

use serenity::all::{GenericChannelId, GuildId, UserId};
use songbird::input::AuxMetadata;
use sqlx::postgres::PgPool;
use std::fmt;
use std::fmt::{Display, Formatter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing;

use crate::db::{metadata::aux_metadata_to_db_structures, Metadata, PlayLog, User};
use crack_common::errors::CrackedError;

// TODO: Make this configurable, and experiment to find a good default.
const CHANNEL_BUF_SIZE: usize = 1024;

/// Data needed to write a metadata entry to the database.
#[derive(Debug, Clone)]
pub struct MetadataMsg {
    pub aux_metadata: AuxMetadata,
    pub user_id: Option<UserId>,
    pub username: Option<String>,
    pub guild_id: GuildId,
    pub channel_id: GenericChannelId,
}

impl Display for MetadataMsg {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "MetadataMsg {{ aux_metadata: {:?}, user_id: {:?}, username: {:?}, guild_id: {:?}, channel_id: {:?} }}",
            self.aux_metadata, self.user_id, self.username, self.guild_id, self.channel_id
        )
    }
}
use crate::db::metadata::MetadataAnd;
/// Writes metadata to the database for a playing track.
async fn write_metadata_pg(
    database_pool: &PgPool,
    data: MetadataMsg,
) -> Result<Metadata, CrackedError> {
    let MetadataMsg {
        aux_metadata,
        user_id,
        username,
        guild_id,
        channel_id,
    } = data;
    let returned_metadata = {
        let MetadataAnd::Track(metadata, _) = match aux_metadata_to_db_structures(
            &aux_metadata,
            guild_id.get() as i64,
            channel_id.get() as i64,
        ) {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("aux_metadata_to_db_structures error: {}", e);
                return Err(CrackedError::Other("aux_metadata_to_db_structures error"));
            },
        };
        let updated_metadata =
            match crate::db::metadata::Metadata::get_or_create(database_pool, &metadata).await {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!("crate::db::metadata::Metadata::create error: {}", e);
                    metadata.clone()
                },
            };

        match User::insert_or_update_user(
            database_pool,
            user_id.map(|x| x.get() as i64).unwrap_or(1),
            username.unwrap_or_default(),
        )
        .await
        {
            Ok(_) => {
                tracing::info!("Users::insert_or_update");
            },
            Err(e) => {
                tracing::error!("Users::insert_or_update error: {}", e);
            },
        };
        match PlayLog::create(
            database_pool,
            user_id.map(|x| x.get() as i64).unwrap_or(1),
            guild_id.get() as i64,
            updated_metadata.id as i64,
        )
        .await
        {
            Ok(x) => {
                tracing::info!("PlayLog::create: {:?}", x);
            },
            Err(e) => {
                tracing::error!("PlayLog::create error: {}", e);
            },
        };
        metadata
    };
    Ok(returned_metadata)
}

/// Writes each message as it arrives, until every sender is dropped.
async fn run(mut receiver: mpsc::Receiver<MetadataMsg>, pool: PgPool) {
    while let Some(message) = receiver.recv().await {
        tracing::trace!("play log writer received {}", message);
        match write_metadata_pg(&pool, message).await {
            Ok(_) => tracing::trace!("Metadata written to database"),
            Err(e) => tracing::warn!("Failed to write metadata to database: {}", e),
        }
    }
}

/// Starts the writer. Returns its sender and the task, which ends once every
/// sender is dropped and the queue is written; the bot never waits for it.
pub fn spawn(pool: PgPool) -> (mpsc::Sender<MetadataMsg>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(CHANNEL_BUF_SIZE);
    let writer = tokio::spawn(run(rx, pool));
    (tx, writer)
}

#[cfg(test)]
mod test {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_writer_records_a_queued_track(pool: PgPool) {
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let (sender, writer) = spawn(pool.clone());
        let data = MetadataMsg {
            aux_metadata: AuxMetadata {
                source_url: Some(url.clone()),
                ..Default::default()
            },
            user_id: Some(UserId::new(100)),
            username: Some("test".to_string()),
            guild_id: GuildId::new(1),
            channel_id: GenericChannelId::new(1),
        };
        sender.send(data).await.unwrap();
        // Dropping the only sender ends the writer once the queue is written.
        drop(sender);
        writer.await.unwrap();
        let metadata = crate::db::metadata::Metadata::get_by_url(&pool, &url)
            .await
            .unwrap()
            .unwrap();
        // test data has id 1
        assert_eq!(metadata.id, 4);
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_get_failed_metadata(pool: PgPool) {
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let metadata = crate::db::metadata::Metadata::get_by_url(&pool, &url)
            .await
            .unwrap();
        assert!(metadata.is_none());
    }
}
