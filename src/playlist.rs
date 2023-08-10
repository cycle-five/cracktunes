use sqlx::{types::chrono, SqlitePool};

use crate::errors::CrackedError;

#[derive(Debug, Default)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub user_id: Option<i64>,
    pub privacy: String,
}

#[derive(Debug, Default)]
pub struct PlaylistTrack {
    pub id: i64,
    pub playlist_id: i64,
    pub track_id: i64,
    pub guild_id: i64,
    pub channel_id: i64,
}

pub struct Metadata {
    pub id: i64,
    pub track: String,
    pub artist: String,
    pub album: String,
    pub date: Option<chrono::NaiveDate>,
    pub channels: i64,
    pub channel: String,
    pub start_time: Option<chrono::NaiveTime>,
    pub duration: Option<chrono::NaiveTime>,
    pub sample_rate: Option<i64>,
    pub source_url: String,
    pub title: String,
    pub thumbnail: String,
}

impl Playlist {
    pub async fn create(
        pool: &SqlitePool,
        name: &str,
        user_id: i64,
    ) -> Result<Playlist, CrackedError> {
        let rec = sqlx::query_as!(
            Playlist,
            "INSERT INTO playlist (name, user_id) VALUES (?, ?) RETURNING id, name, user_id, privacy",
            name,
            user_id
        )
        .fetch_one(pool)
        .await?;

        Ok(rec)
    }

    pub async fn add_track(
        pool: &SqlitePool,
        playlist_id: i32,
        track_id: i32,
        guild_id: i32,
        channel_id: i32,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO playlist_track (playlist_id, track_id, guild_id, channel_id) VALUES (?, ?, ?, ?)",
            playlist_id,
            track_id,
            guild_id,
            channel_id
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    // Additional functions to retrieve, update, and delete playlists and tracks
    // Function to retrieve a playlist by ID
    pub async fn get_playlist_by_id(
        pool: &SqlitePool,
        playlist_id: i64,
    ) -> Result<Playlist, CrackedError> {
        sqlx::query_as!(
            Playlist,
            "SELECT * FROM playlist WHERE id = $1",
            playlist_id
        )
        .fetch_one(pool)
        .await
        .map_err(|e| CrackedError::SQLX(e))
    }

    // Function to update a playlist's name
    pub async fn update_playlist_name(
        pool: &SqlitePool,
        playlist_id: i64,
        new_name: String,
    ) -> Result<Playlist, CrackedError> {
        struct PlaylistOpt {
            id: Option<i64>,
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
            id: r.id.unwrap(),
            name: r.name,
            user_id: r.user_id,
            privacy: r.privacy,
        })
        .map_err(|e| CrackedError::SQLX(e))
    }

    // Function to delete a playlist by ID
    pub async fn delete_playlist(pool: &SqlitePool, playlist_id: i64) -> Result<u64, sqlx::Error> {
        sqlx::query!("DELETE FROM playlist WHERE id = $1", playlist_id)
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
    }

    pub async fn delete_playlist_by_id(
        pool: &SqlitePool,
        playlist_id: i64,
        user_id: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
        DELETE FROM playlist
        WHERE id = ? AND user_id = ?
        "#,
            playlist_id,
            user_id
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
