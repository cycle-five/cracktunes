use std::fmt::{Display, Formatter};

use poise::futures_util::StreamExt;
use poise::serenity_prelude as serenity;
use sqlx::types::chrono::NaiveDateTime;
use sqlx::{Error, PgPool};

use crate::db::Metadata;

/// A play with more dislikes than this is hidden from `/playlog`: one
/// downvote is enough.
pub const PLAYLOG_MAX_DISLIKES: i32 = 0;

#[derive(Debug, Clone)]
pub struct PlayLog {
    pub id: i64,
    pub user_id: i64,
    pub guild_id: i64,
    pub metadata_id: i64,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone)]
pub struct PlayLogQuery {
    pub user_id: Option<i64>,
    pub guild_id: Option<i64>,
    pub limit: Option<i64>,
    pub max_dislikes: Option<i32>,
}

pub trait PgPoolExtPlayLog {
    fn insert_playlog_entry(
        &self,
        user_id: serenity::UserId,
        guild_id: serenity::GuildId,
        metadata_id: i64,
        //) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PlayLog, Error>> + Send>>;
    ) -> impl std::future::Future<Output = Result<PlayLog, Error>>;

    fn get_last_played(
        &self,
        query: &PlayLogQuery,
    ) -> impl std::future::Future<Output = Result<Vec<String>, Error>>;

    fn get_last_played_by_guild(
        &self,
        guild_id: serenity::GuildId,
        limit: i64,
    ) -> impl std::future::Future<Output = Result<Vec<String>, Error>>;
}

#[derive(Debug, Clone)]
struct TitleArtist {
    title: Option<String>,
    artist: Option<String>,
}

impl Display for TitleArtist {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let title = self.title.as_deref().unwrap_or_default();
        let _ = fmt.write_str(title);
        let _ = fmt.write_str(" - ");
        let artist = self.artist.as_deref().unwrap_or_default();
        let _ = fmt.write_str(artist);
        Ok(())
    }
}

impl PgPoolExtPlayLog for PgPool {
    async fn insert_playlog_entry(
        &self,
        user_id: serenity::UserId,
        guild_id: serenity::GuildId,
        metadata_id: i64,
    ) -> Result<PlayLog, Error> {
        PlayLog::create(
            self,
            user_id.get() as i64,
            guild_id.get() as i64,
            metadata_id,
        )
        .await
    }

    async fn get_last_played(&self, query: &PlayLogQuery) -> Result<Vec<String>, Error> {
        match (&query.user_id, &query.guild_id) {
            (Some(user_id), None) => {
                PlayLog::get_last_played_by_user(self, *user_id, query.limit.unwrap_or(i64::MAX))
                    .await
            },
            (None, Some(guild_id)) => {
                PlayLog::get_last_played_by_guild_filter_limit(
                    self,
                    *guild_id,
                    query.max_dislikes.unwrap_or(PLAYLOG_MAX_DISLIKES),
                    query.limit.unwrap_or(i64::MAX),
                )
                .await
            },
            _ => Ok(vec![]),
        }
    }

    async fn get_last_played_by_guild(
        &self,
        guild_id: serenity::GuildId,
        limit: i64,
    ) -> Result<Vec<String>, Error> {
        let query = PlayLogQuery {
            user_id: None,
            guild_id: Some(guild_id.get() as i64),
            limit: Some(limit),
            max_dislikes: None,
        };
        self.get_last_played(&query).await
    }
}

impl PlayLog {
    /// Create a new play log entry.
    pub async fn create(
        conn: &PgPool,
        user_id: i64,
        guild_id: i64,
        metadata_id: i64,
    ) -> Result<Self, Error> {
        let play_log = sqlx::query_as!(
            PlayLog,
            r#"
            INSERT INTO play_log (user_id, guild_id, metadata_id)
            VALUES ($1, $2, $3)
            RETURNING id, user_id, guild_id, metadata_id, created_at
            "#,
            user_id,
            guild_id,
            metadata_id
        )
        .fetch_one(conn)
        .await?;
        Ok(play_log)
    }

    /// Get the last played track for the given user and guild.
    pub async fn get_last_played_by_guild_limit(
        conn: &PgPool,
        guild_id: i64,
        limit: i64,
    ) -> Result<Vec<String>, Error> {
        Self::get_last_played_by_guild_filter_limit(conn, guild_id, 0, limit).await
    }

    /// Get the last played track for the given user and guild.
    pub async fn get_last_played(
        conn: &PgPool,
        user_id: Option<i64>,
        guild_id: Option<i64>,
    ) -> Result<Vec<String>, Error> {
        if user_id.is_none() && guild_id.is_none() || user_id.is_some() && guild_id.is_some() {
            Ok(vec![])
        } else if user_id.is_none() && guild_id.is_some() {
            Self::get_last_played_by_guild(conn, guild_id.unwrap()).await
        } else {
            Self::get_last_played_by_user(conn, user_id.unwrap(), 100).await
        }
    }

    /// Get the last played track for the given guild.
    pub async fn get_last_played_by_guild_filter(
        conn: &PgPool,
        guild_id: i64,
        max_dislikes: i32,
    ) -> Result<Vec<String>, Error> {
        PlayLog::get_last_played_by_guild_filter_limit(conn, guild_id, max_dislikes, 100).await
    }

    pub async fn get_last_played_by_guild_filter_limit(
        conn: &PgPool,
        guild_id: i64,
        max_dislikes: i32,
        limit: i64,
    ) -> Result<Vec<String>, Error> {
        let max_dislikes = if max_dislikes < 0 {
            PLAYLOG_MAX_DISLIKES
        } else {
            max_dislikes
        };
        //let last_played: Vec<TitleArtist> = sqlx::query_as!(
        let mut last_played: Vec<TitleArtist> = Vec::new();
        let mut last_played_stream = sqlx::query_as!(
            TitleArtist,
            r#"
            select title, artist 
            from (play_log
                join metadata on 
                play_log.metadata_id = metadata.id)
                left join track_reaction on play_log.id = track_reaction.play_log_id
            where guild_id = $1 and (track_reaction is null or track_reaction.dislikes <= $2)
            order by play_log.created_at desc limit $3
            "#,
            guild_id,
            max_dislikes,
            limit
        )
        .fetch(conn);
        while let Some(item) = last_played_stream.next().await {
            // Process the item
            last_played.push(item?);
        }
        Ok(last_played.into_iter().map(|t| t.to_string()).collect())
    }

    /// Get the last played track for the given guild.
    pub async fn get_last_played_by_guild(
        conn: &PgPool,
        guild_id: i64,
    ) -> Result<Vec<String>, Error> {
        Self::get_last_played_by_guild_filter(conn, guild_id, PLAYLOG_MAX_DISLIKES).await
    }

    /// Get the last played track for the given guild and return as metadata.
    pub async fn get_last_played_by_guild_metadata(
        conn: &PgPool,
        guild_id: i64,
        limit: i64,
    ) -> Result<Vec<i64>, Error> {
        if limit <= 0 {
            return Ok(vec![]);
        }
        let mut last_played: Vec<Metadata> = Vec::new();
        let mut last_played_stream = sqlx::query_as!(
            Metadata,
            r#"
            select metadata.id, title, artist, album, track, date, channels, channel, start_time, duration, sample_rate, source_url, thumbnail
            from play_log 
            join metadata on 
            play_log.metadata_id = metadata.id 
            where guild_id = $1 order by created_at desc limit $2
            "#,
            guild_id,
            limit
        )
        .fetch(conn);
        while let Some(item) = last_played_stream.next().await {
            // Process the item
            last_played.push(item?);
        }
        Ok(last_played.into_iter().map(|t| t.id as i64).collect())
    }

    /// The `play_log.id` of the newest play in the guild of the track whose
    /// `metadata.source_url` is `source_url`, or `None` if it was never logged.
    ///
    /// 🪤 This is a **play_log** id (what `track_reaction.play_log_id`
    /// references), not a metadata id.
    pub async fn last_play_of_url(
        conn: &PgPool,
        guild_id: i64,
        source_url: &str,
    ) -> Result<Option<i64>, Error> {
        sqlx::query_scalar!(
            r#"
            select play_log.id::bigint as "id!"
            from play_log
            join metadata on play_log.metadata_id = metadata.id
            where play_log.guild_id = $1 and metadata.source_url = $2
            order by play_log.created_at desc, play_log.id desc
            limit 1
            "#,
            guild_id,
            source_url
        )
        .fetch_optional(conn)
        .await
    }

    /// Get the last played track for the given user.
    pub async fn get_last_played_by_user(
        conn: &PgPool,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<String>, Error> {
        if limit < 0 {
            return Ok(vec![]);
        }
        //let last_played: Vec<TitleArtist> = sqlx::query_as!(
        let mut last_played: Vec<TitleArtist> = Vec::new();
        let mut last_played_stream = sqlx::query_as!(
            TitleArtist,
            r#"
            select title, artist 
            from play_log 
            join metadata on 
            play_log.metadata_id = metadata.id 
            where user_id = $1 order by created_at desc limit $2
            "#,
            user_id,
            limit
        )
        .fetch(conn);
        while let Some(item) = last_played_stream.next().await {
            // Process the item
            last_played.push(item?);
        }
        Ok(last_played.into_iter().map(|t| t.to_string()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_playlog(pool: PgPool) -> Result<(), Error> {
        let user_id = 1;
        let guild_id = 1;
        let metadata_id = 1;
        let play_log = PlayLog::create(&pool, user_id, guild_id, metadata_id).await?;
        assert_eq!(play_log.user_id, user_id);
        assert_eq!(play_log.guild_id, guild_id);
        assert_eq!(play_log.metadata_id, metadata_id);
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_playlog_get_last_played(pool: PgPool) -> Result<(), Error> {
        let user_id = 2;
        let guild_id = 1;
        let metadata_id = 1;
        PlayLog::create(&pool, user_id, guild_id, metadata_id).await?;
        let last_played = PlayLog::get_last_played(&pool, None, Some(guild_id)).await?;
        assert_eq!(last_played.len(), 1);
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_playlog_get_last_played_by_user(pool: PgPool) -> Result<(), Error> {
        let user_id = 3;
        let guild_id = 1;
        let metadata_id = 1;
        PlayLog::create(&pool, user_id, guild_id, metadata_id).await?;
        PlayLog::create(&pool, user_id, guild_id, metadata_id + 1).await?;
        PlayLog::create(&pool, user_id, guild_id, metadata_id + 2).await?;
        let last_played = PlayLog::get_last_played(&pool, Some(user_id), None).await?;
        assert_eq!(last_played.len(), 3);
        Ok(())
    }

    /// Insert a play of `source_url` (with a throwaway metadata row) and
    /// return `(metadata_id, play_log_id)`.
    async fn play(pool: &PgPool, guild_id: i64, source_url: &str) -> (i64, i64) {
        let metadata_id: i64 = sqlx::query_scalar(
            "INSERT INTO metadata (source_url, title) VALUES ($1, 't') RETURNING id::bigint",
        )
        .bind(source_url)
        .fetch_one(pool)
        .await
        .unwrap();
        let log = PlayLog::create(pool, 1, guild_id, metadata_id)
            .await
            .unwrap();
        (metadata_id, log.id)
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn last_play_of_url_finds_the_newest_play_of_that_url(pool: PgPool) -> Result<(), Error> {
        // Burn some metadata ids so they differ from the play_log ids.
        for i in 0..3 {
            sqlx::query("INSERT INTO metadata (source_url) VALUES ($1)")
                .bind(format!("https://example.test/filler{i}"))
                .execute(&pool)
                .await?;
        }
        let (_, _first_a) = play(&pool, 1, "https://example.test/a").await;
        let (_, _b) = play(&pool, 1, "https://example.test/b").await;
        // A second play of the same URL (a second metadata row for it, as a
        // re-resolve would make).
        let (meta_a2, newer_a) = play(&pool, 1, "https://example.test/a").await;
        assert_ne!(meta_a2, newer_a, "fixture must make the two ids differ");
        let got = PlayLog::last_play_of_url(&pool, 1, "https://example.test/a").await?;
        assert_eq!(got, Some(newer_a));
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn last_play_of_url_is_none_for_an_unplayed_url(pool: PgPool) -> Result<(), Error> {
        play(&pool, 1, "https://example.test/a").await;
        let got = PlayLog::last_play_of_url(&pool, 1, "https://example.test/never").await?;
        assert_eq!(got, None);
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn last_play_of_url_is_scoped_to_the_guild(pool: PgPool) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, guild_name, prefix) VALUES (2, 'two', 'r!')",
        )
        .execute(&pool)
        .await?;
        let (_, mine) = play(&pool, 1, "https://example.test/a").await;
        let (_, _theirs_newer) = play(&pool, 2, "https://example.test/a").await;
        let got = PlayLog::last_play_of_url(&pool, 1, "https://example.test/a").await?;
        assert_eq!(got, Some(mine));
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_downvoted_play_is_hidden_from_playlog(pool: PgPool) -> Result<(), Error> {
        use crate::db::TrackReaction;
        let (_, keep1) = play(&pool, 1, "https://example.test/keep1").await;
        let (_, bad) = play(&pool, 1, "https://example.test/bad").await;
        let (_, keep2) = play(&pool, 1, "https://example.test/keep2").await;
        let _ = (keep1, keep2);
        sqlx::query("UPDATE metadata SET title = 'keep1' WHERE source_url LIKE '%keep1'")
            .execute(&pool)
            .await?;
        sqlx::query("UPDATE metadata SET title = 'bad' WHERE source_url LIKE '%/bad'")
            .execute(&pool)
            .await?;
        sqlx::query("UPDATE metadata SET title = 'keep2' WHERE source_url LIKE '%keep2'")
            .execute(&pool)
            .await?;
        let before = PlayLog::get_last_played(&pool, None, Some(1)).await?;
        assert_eq!(before.len(), 3);
        let id = bad as i32;
        TrackReaction::insert(&pool, id).await.unwrap();
        TrackReaction::add_dislike(&pool, id).await.unwrap();
        let after = PlayLog::get_last_played(&pool, None, Some(1)).await?;
        assert_eq!(after.len(), 2, "{after:?}");
        assert!(after.iter().all(|t| !t.starts_with("bad")), "{after:?}");
        assert!(after.iter().any(|t| t.starts_with("keep1")));
        assert!(after.iter().any(|t| t.starts_with("keep2")));
        Ok(())
    }
}
