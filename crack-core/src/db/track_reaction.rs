use crate::CrackedError;
use chrono::NaiveDateTime;
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
    /// Insert a new track reaction.
    pub async fn insert(pool: &PgPool, play_log_id: i32) -> Result<TrackReaction, CrackedError> {
        sqlx::query_as!(
            TrackReaction,
            r#"
            INSERT INTO track_reaction
                (play_log_id)
            VALUES
                ($1)
            ON CONFLICT (play_log_id) DO NOTHING
            RETURNING play_log_id, likes, dislikes, skip_votes, created_at"#,
            play_log_id,
        )
        .fetch_one(pool)
        .await
        .map_err(std::convert::Into::into)
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
        .map_err(std::convert::Into::into)
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
        .map_err(std::convert::Into::into)
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
        .map_err(std::convert::Into::into)
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
        .map_err(std::convert::Into::into)
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
        .map_err(std::convert::Into::into)
    }
}
