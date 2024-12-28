use crate::db::{user::User, Metadata, MetadataRead};
use crate::messages::{PLAYLISTS, PLAYLIST_LIST_EMPTY};
use crate::{CrackedError, EMBED_PAGE_SIZE};
use serenity::all::CreateEmbed;
use sqlx::{postgres::PgQueryResult, query, PgPool};

/// Playlist db structure (does not old the tracks)
#[derive(Debug, Default)]
pub struct Playlist {
    pub id: i32,
    pub name: String,
    pub user_id: Option<i64>,
    pub privacy: String,
}

/// PlaylistTrack db structure.
#[derive(Debug, Default)]
pub struct PlaylistTrack {
    pub id: i64,
    pub playlist_id: i32,
    pub metadata_id: i32,
    pub guild_id: Option<i64>,
    pub channel_id: Option<i64>,
}

/// Implementation of the Playlist struct for writing to the database
impl Playlist {
    /// Create a new playlist for a user.
    pub async fn create(pool: &PgPool, name: &str, user_id: i64) -> Result<Playlist, CrackedError> {
        if User::get_user(pool, user_id).await.is_none() {
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

    /// Add a track to a playlist.
    pub async fn add_track(
        pool: &PgPool,
        playlist_id: i32,
        metadata_id: i32,
        guild_id: i64,
        channel_id: i64,
    ) -> Result<PgQueryResult, sqlx::Error> {
        query!(
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

    /// Retreive playlists by user ID
    pub async fn get_playlists_by_user_id(
        pool: &PgPool,
        user_id: i64,
    ) -> Result<Vec<Playlist>, CrackedError> {
        sqlx::query_as!(
            Playlist,
            "SELECT * FROM playlist WHERE user_id = $1",
            user_id
        )
        .fetch_all(pool)
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
    pub async fn delete_playlist(
        pool: &PgPool,
        playlist_id: i32,
    ) -> Result<PgQueryResult, sqlx::Error> {
        let _ = sqlx::query!(
            r#"
            DELETE FROM playlist_track
            WHERE playlist_id = $1"#,
            playlist_id
        )
        .execute(pool)
        .await?;
        sqlx::query!(
            r#"
            DELETE FROM playlist
            WHERE id = $1"#,
            playlist_id,
        )
        .execute(pool)
        .await
    }

    /// Delete a playlist by playlist ID and user ID
    pub async fn delete_playlist_by_id(
        pool: &PgPool,
        playlist_id: i32,
        _user_id: i64,
    ) -> Result<PgQueryResult, sqlx::Error> {
        Self::delete_playlist(pool, playlist_id).await
    }

    /// Get all tracks in a playlist
    pub async fn get_tracks_in_playlist(
        pool: &PgPool,
        playlist_id: i32,
    ) -> Result<Vec<PlaylistTrack>, sqlx::Error> {
        sqlx::query_as!(
            PlaylistTrack,
            r#"
                SELECT * FROM playlist_track
                WHERE playlist_id = $1"#,
            playlist_id
        )
        .fetch_all(pool)
        .await
    }

    /// Get the metadata for the tracks for a playlist. This is what is needed
    /// to queue the playlist.
    pub async fn get_track_metadata_for_playlist(
        pool: &PgPool,
        playlist_id: i32,
    ) -> Result<Vec<Metadata>, sqlx::Error> {
        sqlx::query_as!(
            MetadataRead,
            r#"
                SELECT
                    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
                FROM
                    (metadata INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id)
                WHERE
                    playlist_track.playlist_id = $1"#,
            playlist_id,
        )
        .fetch_all(pool)
        .await
        .map(|r| r.into_iter().map(|r| r.into()).collect())
    }

    /// Gets the metadata for a playlist for a user by playlist name.
    pub async fn get_track_metadata_for_playlist_name(
        pool: &PgPool,
        playlist_name: String,
        user_id: i64,
    ) -> Result<Vec<Metadata>, sqlx::Error> {
        sqlx::query_as!(
            MetadataRead,
            r#"
                SELECT
                    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
                FROM
                    (metadata INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id INNER JOIN playlist ON playlist_track.playlist_id = playlist.id)
                WHERE playlist.name = $1 AND playlist.user_id = $2"#,
            playlist_name,
            user_id,
        )
        .fetch_all(pool)
        .await
        .map(|r| r.into_iter().map(Into::into).collect())
    }

    /// Delete a playlist by playlist name and user ID
    pub async fn delete_playlist_by_name(
        pool: &PgPool,
        playlist_name: String,
        user_id: i64,
    ) -> Result<(), sqlx::Error> {
        struct I32Wrapper {
            id: i32,
        }
        let I32Wrapper { id: playlist_id } = sqlx::query_as!(
            I32Wrapper,
            r#"
                SELECT id FROM playlist
                WHERE name = $1 AND user_id = $2 
            "#,
            playlist_name,
            user_id,
        )
        .fetch_one(pool)
        .await?;

        Self::delete_playlist(pool, playlist_id).await.map(|_| ())
    }
}

pub async fn build_playlist_list_embed(playlists: &[Playlist], page: usize) -> CreateEmbed {
    use std::fmt::Write;
    let content = if !playlists.is_empty() {
        let start_idx = EMBED_PAGE_SIZE * page;
        let playlists: Vec<&Playlist> = playlists.iter().skip(start_idx).take(10).collect();

        let mut description = String::new();

        for (i, &playlist) in playlists.iter().enumerate() {
            let _ = writeln!(
                description,
                // "`{}.` [{}]({})",
                "`{}.` {} ({})",
                i + start_idx + 1,
                playlist.name,
                playlist.id
            );
        }

        description
    } else {
        PLAYLIST_LIST_EMPTY.to_string()
    };

    CreateEmbed::default().title(PLAYLISTS).description(content)
    //     .footer(CreateEmbedFooter::new(format!(
    //         "{} {} {} {}",
    //         QUEUE_PAGE,
    //         page + 1,
    //         QUEUE_PAGE_OF,
    //         calculate_num_pages(playlists),
    //     )))
}
