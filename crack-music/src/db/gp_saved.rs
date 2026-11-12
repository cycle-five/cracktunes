//! Private GP bookmarks backed by playlists. All writes for one owner serialize
//! on the user row, including the URL duplicate check and first-list creation.
use super::{Metadata, MetadataRead, User};
use crack_common::{errors::CrackedError, CrackedResult};
use crack_types::NewAuxMetadata;
use songbird::input::AuxMetadata;
use sqlx::PgPool;

pub async fn save(
    pool: &PgPool,
    owner: i64,
    username: String,
    guild: i64,
    channel: i64,
    aux: &AuxMetadata,
) -> CrackedResult<bool> {
    let url = aux
        .source_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .ok_or(CrackedError::Other("This song has no playable URL."))?;
    User::insert_or_update_user(pool, owner, username).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT id FROM public.user WHERE id = $1 FOR UPDATE")
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    let existing: Option<i32> =
        sqlx::query_scalar("SELECT playlist_id FROM gp_saved_playlist WHERE user_id = $1")
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?;
    let playlist = match existing {
        Some(id) => id,
        None => {
            let id: i32 = sqlx::query_scalar(
                "INSERT INTO playlist (name, user_id) VALUES ('GP saved', $1) RETURNING id",
            )
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query("INSERT INTO gp_saved_playlist (user_id, playlist_id) VALUES ($1, $2)")
                .bind(owner)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            id
        },
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM playlist_track pt
         JOIN metadata m ON m.id = pt.metadata_id
         WHERE pt.playlist_id = $1 AND m.source_url = $2)",
    )
    .bind(playlist)
    .bind(url)
    .fetch_one(&mut *tx)
    .await?;
    if exists {
        tx.commit().await?;
        return Ok(false);
    }
    // Store our own full-song metadata: never reuse a row containing a clip offset.
    let super::metadata::MetadataAnd::Track(metadata, _) =
        super::metadata::aux_metadata_to_db_structures(aux, guild, channel)?;
    let id: i32 = sqlx::query_scalar(
        "INSERT INTO metadata
         (source_url, title, track, artist, album, thumbnail, start_time,
          duration, date, channels, channel, sample_rate)
         VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $9, $10, $11)
         RETURNING id",
    )
    .bind(url)
    .bind(&metadata.title)
    .bind(&metadata.track)
    .bind(&metadata.artist)
    .bind(&metadata.album)
    .bind(&metadata.thumbnail)
    .bind(metadata.duration)
    .bind(metadata.date)
    .bind(metadata.channels)
    .bind(&metadata.channel)
    .bind(metadata.sample_rate)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO playlist_track (playlist_id, metadata_id, guild_id, channel_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(playlist)
    .bind(id)
    .bind(guild)
    .bind(channel)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn load(pool: &PgPool, owner: i64) -> CrackedResult<Vec<NewAuxMetadata>> {
    let rows = sqlx::query_as::<_, MetadataRead>(
        "SELECT m.* FROM metadata m
         JOIN playlist_track pt ON pt.metadata_id = m.id
         JOIN gp_saved_playlist p ON p.playlist_id = pt.playlist_id
         WHERE p.user_id = $1 ORDER BY pt.id",
    )
    .bind(owner)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let mut aux = super::metadata::aux_metadata_from_db(&Metadata::from(row))?;
            aux.start_time = None;
            Ok(NewAuxMetadata(aux))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use songbird::input::AuxMetadata;
    use sqlx::PgPool;
    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    async fn save_load_private_and_idempotent(pool: PgPool) {
        let aux = AuxMetadata {
            source_url: Some("https://example.com/song".into()),
            title: Some("Song".into()),
            start_time: Some(std::time::Duration::from_secs(30)),
            ..Default::default()
        };
        assert!(save(&pool, 100, "listener".into(), 1, 2, &aux)
            .await
            .unwrap());
        assert!(!save(&pool, 100, "listener".into(), 1, 2, &aux)
            .await
            .unwrap());
        let tracks = load(&pool, 100).await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].0.source_url, aux.source_url);
        assert!(tracks[0].0.start_time.is_none());
        assert!(load(&pool, 200).await.unwrap().is_empty());
        assert!(save(&pool, 200, "other".into(), 1, 2, &aux).await.unwrap());
        assert_eq!(load(&pool, 100).await.unwrap().len(), 1);
        let next = AuxMetadata {
            source_url: Some("https://example.com/next".into()),
            ..Default::default()
        };
        let (a, b) = tokio::join!(
            save(&pool, 100, "listener".into(), 1, 2, &next),
            save(&pool, 100, "listener".into(), 1, 2, &next)
        );
        assert_ne!(a.unwrap(), b.unwrap());
        assert_eq!(load(&pool, 100).await.unwrap().len(), 2);
    }
}
