use serenity::all::{ChannelId, GuildId, UserId};
use songbird::input::AuxMetadata;
use sqlx::postgres::PgPool;
use std::fmt;
use std::fmt::{Display, Formatter};
use tokio::sync::mpsc;
use tracing;

use crate::db::{metadata::aux_metadata_to_db_structures, Metadata, PlayLog, User};
use crate::CrackedError;

// TODO: Make this configurable, and experiment to find a good default.
const CHANNEL_BUF_SIZE: usize = 1024;

/// Data needed to write a metadata entry to the database.
#[derive(Debug, Clone)]
pub struct MetadataMsg {
    pub aux_metadata: AuxMetadata,
    pub user_id: UserId,
    pub username: String,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
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
pub async fn write_metadata_pg(
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

        match User::insert_or_update_user(database_pool, user_id.get() as i64, username).await {
            Ok(_) => {
                tracing::info!("Users::insert_or_update");
            },
            Err(e) => {
                tracing::error!("Users::insert_or_update error: {}", e);
            },
        };
        match PlayLog::create(
            database_pool,
            user_id.get() as i64,
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

/// Run a worker that writes metadata to the database.
pub async fn run_db_worker(mut receiver: mpsc::Receiver<MetadataMsg>, pool: PgPool) {
    while let Some(message) = receiver.recv().await {
        tracing::trace!("Received message in run_db_worker: {}", message);
        match write_metadata_pg(&pool, message).await {
            Ok(_) => tracing::trace!("Metadata written to database"),
            Err(e) => tracing::warn!("Failed to write metadata to database: {}", e),
        }
    }
}

/// Setup the workers to handle db writes asynchronously during runtime.
pub async fn setup_workers(pool: PgPool) -> mpsc::Sender<MetadataMsg> {
    let (tx, rx) = mpsc::channel(CHANNEL_BUF_SIZE);
    let pool = pool.clone();
    tokio::spawn(async move {
        run_db_worker(rx, pool).await;
    });
    tx
}

#[cfg(test)]
mod test {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_workers(pool: PgPool) {
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let sender = setup_workers(pool.clone()).await;
        let data = MetadataMsg {
            aux_metadata: AuxMetadata {
                source_url: Some(url.clone()),
                ..Default::default()
            },
            user_id: UserId::new(1),
            username: "test".to_string(),
            guild_id: GuildId::new(1),
            channel_id: ChannelId::new(1),
        };
        sender.send(data).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let metadata = crate::db::metadata::Metadata::get_by_url(&pool, &url)
            .await
            .unwrap()
            .unwrap();
        // test data has id 1
        assert_eq!(metadata.id, 2);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_get_failed_metadata(pool: PgPool) {
        let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let metadata = crate::db::metadata::Metadata::get_by_url(&pool, &url)
            .await
            .unwrap();
        assert!(metadata.is_none());
    }
}
