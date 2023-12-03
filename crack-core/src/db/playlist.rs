use crate::db::user::User;
use ::chrono::Duration;
use songbird::tracks::TrackHandle;
use sqlx::{
    postgres::{types::PgInterval, PgQueryResult},
    types::chrono::{self},
    PgPool,
};

use crate::CrackedError;

#[derive(Debug, Default)]
pub struct Playlist {
    pub id: i32,
    pub name: String,
    pub user_id: Option<i64>,
    pub privacy: String,
}

#[derive(Debug, Default)]
pub struct PlaylistTrack {
    pub id: i64,
    pub playlist_id: i32,
    pub metadata_id: i32,
    pub guild_id: Option<i64>,
    pub channel_id: Option<i64>,
}

pub struct Metadata {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: Option<Duration>,
    pub duration: Option<Duration>,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

pub struct MetadataRead {
    pub id: i32,
    pub track: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub date: Option<chrono::NaiveDate>,
    pub channels: Option<i16>,
    pub channel: Option<String>,
    pub start_time: Option<PgInterval>,
    pub duration: Option<PgInterval>,
    pub sample_rate: Option<i32>,
    pub source_url: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

impl Metadata {
    pub async fn create(pool: &PgPool, in_metadata: Metadata) -> Result<Metadata, CrackedError> {
        let r = sqlx::query_as!(
            MetadataRead,
            r#"INSERT INTO
                metadata (track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                RETURNING id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
            "#,
            in_metadata.track,
            in_metadata.artist,
            in_metadata.album,
            in_metadata.date,
            in_metadata.channels.map(|x| i16::try_from(x).unwrap()),
            in_metadata.channel,
            in_metadata.start_time.map(|x| PgInterval::try_from(x.to_std().unwrap()).unwrap()),
            in_metadata.duration.map(|x| PgInterval::try_from(x.to_std().unwrap()).unwrap()),
            in_metadata.sample_rate,
            in_metadata.source_url,
            in_metadata.title,
            in_metadata.thumbnail,
        )
        .fetch_one(pool)
        .await
        .map_err(CrackedError::SQLX)?;
        Ok(Metadata {
            id: r.id,
            track: r.track,
            artist: r.artist,
            album: r.album,
            date: r.date,
            channels: r.channels,
            channel: r.channel,
            start_time: r.start_time.map(|x| {
                Duration::from_std(std::time::Duration::from_micros(
                    x.microseconds.unsigned_abs(),
                ))
                .unwrap()
            }),
            duration: r.duration.map(|x| {
                Duration::from_std(std::time::Duration::from_micros(
                    x.microseconds.unsigned_abs(),
                ))
                .unwrap()
            }),
            sample_rate: r.sample_rate,
            source_url: r.source_url,
            title: r.title,
            thumbnail: r.thumbnail,
        })
    }
}

impl Playlist {
    pub async fn create(pool: &PgPool, name: &str, user_id: i64) -> Result<Playlist, CrackedError> {
        if User::get_user(pool, user_id).await.is_none() {
            // match User::insert_user(pool, user_id, "FAKENAME".to_string()).await {
            //     Ok(_) => (),
            //     Err(e) => {
            //         return Err(CrackedError::SQLX(e));
            //     }
            // }
            return Err(CrackedError::Other(
                "(playlist::create) User does not exist",
            ));
        }
        let rec = sqlx::query_as!(
            Playlist,
            "INSERT INTO playlist (name, user_id) VALUES ($1, $2) RETURNING id, name, user_id, privacy",
            name,
            user_id,
        )
        .fetch_one(pool)
        .await?;

        Ok(rec)
    }

    pub async fn add_track(
        pool: &PgPool,
        playlist_id: i32,
        metadata_id: i32,
        guild_id: i64,
        channel_id: i64,
    ) -> sqlx::Result<PgQueryResult> {
        sqlx::query!(
            "INSERT INTO playlist_track (playlist_id, metadata_id, guild_id, channel_id) VALUES ($1, $2, $3, $4)",
            playlist_id,
            metadata_id,
            guild_id,
            channel_id
        )
        .execute(pool)
        .await
    }

    // Additional functions to retrieve, update, and delete playlists and tracks

    /// Reterive a playlist by ID
    pub async fn get_playlist_by_id(
        pool: &PgPool,
        playlist_id: i32,
    ) -> Result<Playlist, CrackedError> {
        sqlx::query_as!(
            Playlist,
            "SELECT * FROM playlist WHERE id = $1",
            playlist_id
        )
        .fetch_one(pool)
        .await
        .map_err(CrackedError::SQLX)
    }

    /// Reterive a playlist by name and user ID.
    pub async fn get_playlist_by_name(
        pool: &PgPool,
        name: String,
        user_id: i64,
    ) -> Result<Playlist, CrackedError> {
        sqlx::query_as!(
            Playlist,
            "SELECT * FROM playlist WHERE user_id = $1 and name = $2",
            user_id,
            name
        )
        .fetch_one(pool)
        .await
        .map_err(CrackedError::SQLX)
    }

    /// Function to update a playlist's name
    pub async fn update_playlist_name(
        pool: &PgPool,
        playlist_id: i32,
        new_name: String,
    ) -> Result<Playlist, CrackedError> {
        struct PlaylistOpt {
            id: i32,
            name: String,
            user_id: Option<i64>,
            privacy: String,
        }
        let res = sqlx::query_as!(
            PlaylistOpt,
            "UPDATE playlist SET name = $1 WHERE id = $2 RETURNING id, name, user_id, privacy",
            new_name,
            playlist_id
        )
        .fetch_one(pool)
        .await;

        res.map(|r| Playlist {
            id: r.id,
            name: r.name,
            user_id: r.user_id,
            privacy: r.privacy,
        })
        .map_err(CrackedError::SQLX)
    }

    /// Delete a playlist by playlist ID
    pub async fn delete_playlist(pool: &PgPool, playlist_id: i32) -> Result<u64, sqlx::Error> {
        sqlx::query!("DELETE FROM playlist WHERE id = $1", playlist_id)
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
    }

    /// Delete a playlist by playlist ID and user ID
    pub async fn delete_playlist_by_id(
        pool: &PgPool,
        playlist_id: i32,
        user_id: i64,
    ) -> Result<PgQueryResult, sqlx::Error> {
        sqlx::query!(
            r#"
        DELETE FROM playlist
        WHERE id = $1 AND user_id = $2"#,
            playlist_id,
            user_id
        )
        .execute(pool)
        .await
    }

    pub async fn delete_playlist_by_name(
        pool: &PgPool,
        playlist_name: String,
        user_id: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
        DELETE FROM playlist
        WHERE name = $1 AND user_id = $2 
        "#,
            playlist_name,
            user_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

pub async fn track_handle_to_db_structures(
    _pool: &PgPool,
    _track_handle: TrackHandle,
    _playlist_id: i64,
    _guild_id: i64,
    _channel_id: i64,
) -> Result<(Metadata, PlaylistTrack), CrackedError> {
    // 1. Extract metadata from TrackHandle
    Err(CrackedError::Other("not implemented"))
    // track_handle.action(View).await?;
    // track_handle.get
    // let track = track_handle.metadata().track.clone();
    // let title = track_handle.metadata().title.clone();
    // let artist = track_handle.metadata().artist.clone();
    // let album = Some("".to_string());
    // let date = track_handle
    //     .metadata()
    //     .date
    //     .clone()
    //     .map(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap_or_default());
    // let channels = track_handle.metadata().channels;
    // let channel = Some(channel_id);
    // let start_time = track_handle
    //     .metadata()
    //     .start_time
    //     .map(|d| d.as_secs() as i64);
    // let duration = track_handle.metadata().duration.map(|d| d.as_secs() as i64);
    // let sample_rate = track_handle.metadata().sample_rate.map(i64::from);
    // let source_url = track_handle.metadata().source_url.clone();
    // let thumbnail = track_handle.metadata().thumbnail.clone();

    // let metadata = sqlx::query_as!(
    //     Metadata,
    //     r#"INSERT INTO
    //         metadata (track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail)
    //         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    //         RETURNING id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
    //         "#,
    //     track,
    //     artist,
    //     album,
    //     date,
    //     channels,
    //     channel,
    //     start_time,
    //     duration,
    //     sample_rate,
    //     source_url,
    //     title,
    //     thumbnail
    // )
    // .fetch_one(pool)
    // .await
    // .map_err(CrackedError::SQLX)?;

    // let guild_id_opt = Some(guild_id);
    // let channel_id_opt = Some(channel_id);
    // // 3. Populate the PlaylistTrack structure
    // let playlist_track = sqlx::query_as!(
    //     PlaylistTrack,
    //     r#"INSERT INTO playlist_track
    //         (playlist_id, metadata_id, guild_id, channel_id)
    //         VALUES (?, ?, ?, ?)
    //         RETURNING id, playlist_id, metadata_id, guild_id, channel_id
    //         "#,
    //     playlist_id,
    //     metadata.id,
    //     guild_id_opt,
    //     channel_id_opt
    // )
    // .fetch_one(pool)
    // .await
    // .map_err(CrackedError::SQLX)?;

    // Ok((metadata, playlist_track))
}
