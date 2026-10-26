use chrono::NaiveDateTime;
use crack_common::errors::CrackedError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackReaction {
    pub play_log_id: i64,
    pub likes: i32,
    pub dislikes: i32,
    pub skip_votes: i32,
    pub created_at: NaiveDateTime,
}

impl TrackReaction {
    /// Insert a new track reaction, or return the existing one for this play.
    /// Never errors on a repeat: `add_dislike` and friends rely on the row
    /// existing, and a second reaction to the same play is normal.
    pub async fn insert(pool: &PgPool, play_log_id: i32) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            INSERT INTO track_reaction
                (play_log_id)
            VALUES
                ($1)
            ON CONFLICT (play_log_id) DO UPDATE SET play_log_id = EXCLUDED.play_log_id
            RETURNING play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id,
        )
        .fetch_one(pool)
        .await
        .map_err(|e| e.into())
    }

    /// Get the track reaction.
    pub async fn get_track_reaction(
        pool: &PgPool,
        play_log_id: i32,
    ) -> Result<Option<TrackReaction>, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            "SELECT * FROM track_reaction WHERE play_log_id = $1",
            play_log_id,
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| e.into())
    }

    /// Update the track reaction.
    pub async fn update_track_reaction(
        pool: &PgPool,
        play_log_id: i32,
        likes: i32,
        dislikes: i32,
        skip_votes: i32,
    ) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            UPDATE
                track_reaction
            SET
                likes = $2, dislikes = $3, skip_votes = $4
            WHERE 
                play_log_id = $1
            RETURNING 
                play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id,
            likes,
            dislikes,
            skip_votes,
        )
        .fetch_one(pool)
        .await
        .map_err(|e| e.into())
    }

    /// Add a like to the track reaction.
    /// Returns the updated track reaction.
    pub async fn add_like(pool: &PgPool, play_log_id: i32) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            UPDATE
                track_reaction
            SET
                likes = likes + 1
            WHERE 
                play_log_id = $1
            RETURNING 
                play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id,
        )
        .fetch_one(pool)
        .await
        .map_err(|e| e.into())
    }

    /// Add a dislike to the track reaction.
    /// Returns the updated track reaction.
    pub async fn add_dislike(
        pool: &PgPool,
        play_log_id: i32,
    ) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            UPDATE
                track_reaction
            SET
                dislikes = dislikes + 1
            WHERE 
                play_log_id = $1
            RETURNING 
                play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id.into(),
        )
        .fetch_one(pool)
        .await
        .map_err(|e| e.into())
    }

    /// Add a skipvote to the track reaction.
    /// Returns the updated track reaction.
    pub async fn add_skipvote(
        pool: &PgPool,
        play_log_id: i32,
    ) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            UPDATE
                track_reaction
            SET
                skip_votes = skip_votes + 1
            WHERE 
                play_log_id = $1
            RETURNING 
                play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id,
        )
        .fetch_one(pool)
        .await
        .map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn downvoting_twice_counts_two_dislikes(pool: PgPool) -> Result<(), CrackedError> {
        let metadata_id: i64 = sqlx::query_scalar(
            "INSERT INTO metadata (source_url) VALUES ('https://example.test/a') RETURNING id::bigint",
        )
        .fetch_one(&pool)
        .await?;
        let log = crate::db::PlayLog::create(&pool, 1, 1, metadata_id).await?;
        let id = log.id as i32;
        // The same pair `Data::downvote_track` runs, twice on one play.
        TrackReaction::insert(&pool, id).await?;
        TrackReaction::add_dislike(&pool, id).await?;
        TrackReaction::insert(&pool, id).await?;
        let reaction = TrackReaction::add_dislike(&pool, id).await?;
        assert_eq!(reaction.dislikes, 2);
        Ok(())
    }
}
