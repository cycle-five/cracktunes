use crate::db::metadata::{aux_metadata_to_db_structures, MetadataAnd};
use crate::db::{user::User, Metadata, MetadataRead};
use crate::CrackedError;
use songbird::input::AuxMetadata;
use sqlx::{postgres::PgQueryResult, query, PgPool};

/// What [`Playlist::save_onto`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedTrackStatus {
    /// A new row. The song was not on this list.
    Saved,
    /// The list already had this track. Nothing was inserted.
    AlreadyThere,
}

/// A track to put on someone's list. The playlist is created, private, on the
/// first save, and a user row is written first so a listener who has never
/// queued anything can still keep a song.
pub struct SaveOnto<'a> {
    pub user_id: i64,
    pub username: &'a str,
    pub playlist_name: &'a str,
    pub metadata: &'a AuxMetadata,
    pub guild_id: i64,
    pub channel_id: i64,
}

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

    /// The user's list of this name, in the order tracks were saved. Empty when
    /// they have no such list. Another user's list is never returned: the name
    /// is only ever matched together with `user_id`.
    pub async fn load_for_user(
        pool: &PgPool,
        playlist_name: &str,
        user_id: i64,
    ) -> Result<Vec<Metadata>, CrackedError> {
        sqlx::query_as!(
            MetadataRead,
            r#"
                SELECT
                    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
                FROM
                    metadata
                    INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id
                    INNER JOIN playlist ON playlist_track.playlist_id = playlist.id
                WHERE playlist.name = $1 AND playlist.user_id = $2
                ORDER BY playlist_track.id
            "#,
            playlist_name,
            user_id,
        )
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
        .map_err(CrackedError::SQLX)
    }

    /// Insert the track unless this list already has it.
    async fn add_track_once(
        pool: &PgPool,
        playlist_id: i32,
        metadata_id: i32,
        guild_id: i64,
        channel_id: i64,
    ) -> Result<bool, CrackedError> {
        let res = query!(
            r#"
                INSERT INTO playlist_track (playlist_id, metadata_id, guild_id, channel_id)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (playlist_id, metadata_id) DO NOTHING
            "#,
            playlist_id,
            metadata_id,
            guild_id,
            channel_id
        )
        .execute(pool)
        .await
        .map_err(CrackedError::SQLX)?;
        Ok(res.rows_affected() > 0)
    }

    /// Add `metadata` to `playlist_name` for this user. Creates the user and a
    /// private list on the first save. A second save of the same track is a
    /// no-op.
    pub async fn save_onto(
        pool: &PgPool,
        req: SaveOnto<'_>,
    ) -> Result<SavedTrackStatus, CrackedError> {
        User::insert_or_update_user(pool, req.user_id, req.username.to_string())
            .await
            .map_err(CrackedError::SQLX)?;
        let playlist = match Self::get_playlist_by_name(
            pool,
            req.playlist_name.to_string(),
            req.user_id,
        )
        .await
        {
            Ok(playlist) => playlist,
            Err(CrackedError::SQLX(sqlx::Error::RowNotFound)) => {
                Self::create(pool, req.playlist_name, req.user_id).await?
            },
            Err(e) => return Err(e),
        };
        let MetadataAnd::Track(in_metadata, _) =
            aux_metadata_to_db_structures(req.metadata, req.guild_id, req.channel_id)?;
        let metadata = Metadata::get_or_create(pool, &in_metadata).await?;
        let inserted =
            Self::add_track_once(pool, playlist.id, metadata.id, req.guild_id, req.channel_id)
                .await?;
        Ok(if inserted {
            SavedTrackStatus::Saved
        } else {
            SavedTrackStatus::AlreadyThere
        })
    }
}

#[cfg(test)]
mod test {
    use super::{Playlist, SaveOnto, SavedTrackStatus};
    use crate::db::User;
    use songbird::input::AuxMetadata;
    use sqlx::PgPool;
    use std::time::Duration;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn song(url: &str, title: &str) -> AuxMetadata {
        AuxMetadata {
            title: Some(title.to_string()),
            artist: Some("Someone".into()),
            source_url: Some(url.to_string()),
            duration: Some(Duration::from_secs(214)),
            start_time: None,
            ..AuxMetadata::default()
        }
    }

    /// Save writes a private list for a user who had no row yet, a second save
    /// does not insert again, and loading it back is that one full track.
    /// Someone else's list stays empty.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn save_then_load_does_not_duplicate(pool: PgPool) {
        let url = "https://www.youtube.com/watch?v=fullsong";
        let meta = song(url, "Full Song");
        assert!(
            User::get_user(&pool, 42).await.is_none(),
            "the saver starts with no user row"
        );

        let first = Playlist::save_onto(
            &pool,
            SaveOnto {
                user_id: 42,
                username: "ada",
                playlist_name: "gp saved",
                metadata: &meta,
                guild_id: 7,
                channel_id: 8,
            },
        )
        .await
        .unwrap();
        assert_eq!(first, SavedTrackStatus::Saved);

        let second = Playlist::save_onto(
            &pool,
            SaveOnto {
                user_id: 42,
                username: "ada",
                playlist_name: "gp saved",
                metadata: &meta,
                guild_id: 9,
                channel_id: 10,
            },
        )
        .await
        .unwrap();
        assert_eq!(second, SavedTrackStatus::AlreadyThere);

        let loaded = Playlist::load_for_user(&pool, "gp saved", 42)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1, "pressing save twice is one row");
        assert_eq!(loaded[0].source_url.as_deref(), Some(url));
        assert_eq!(loaded[0].title.as_deref(), Some("Full Song"));
        assert_eq!(loaded[0].duration, 214, "the whole song, not a clip");
        assert_eq!(loaded[0].start_time, 0, "playback starts at the beginning");

        let playlist = Playlist::get_playlist_by_name(&pool, "gp saved".into(), 42)
            .await
            .unwrap();
        assert_eq!(playlist.privacy, "private");
        assert!(User::get_user(&pool, 42).await.is_some());

        // A second song keeps save order. Another user saving the same URL
        // gets their own copy and cannot see the first user's list.
        let other_song = song("https://www.youtube.com/watch?v=second", "Second");
        Playlist::save_onto(
            &pool,
            SaveOnto {
                user_id: 42,
                username: "ada",
                playlist_name: "gp saved",
                metadata: &other_song,
                guild_id: 7,
                channel_id: 8,
            },
        )
        .await
        .unwrap();
        let loaded = Playlist::load_for_user(&pool, "gp saved", 42)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .iter()
                .map(|m| m.title.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["Full Song", "Second"]
        );

        Playlist::save_onto(
            &pool,
            SaveOnto {
                user_id: 99,
                username: "bob",
                playlist_name: "gp saved",
                metadata: &meta,
                guild_id: 7,
                channel_id: 8,
            },
        )
        .await
        .unwrap();
        let theirs = Playlist::load_for_user(&pool, "gp saved", 99)
            .await
            .unwrap();
        assert_eq!(theirs.len(), 1);
        assert!(
            Playlist::load_for_user(&pool, "gp saved", 100)
                .await
                .unwrap()
                .is_empty(),
            "a user who never saved has nothing to load"
        );
        assert_eq!(
            Playlist::load_for_user(&pool, "gp saved", 42)
                .await
                .unwrap()
                .len(),
            2,
            "bob's save did not land on ada's list"
        );
    }
}
