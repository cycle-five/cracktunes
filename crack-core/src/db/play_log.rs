use std::fmt::{Display, Formatter};

use poise::futures_util::StreamExt;
use sqlx::types::chrono::NaiveDateTime;
use sqlx::{Error, PgPool};

#[derive(Debug, Clone)]
pub struct PlayLog {
    pub id: i64,
    pub user_id: i64,
    pub guild_id: i64,
    pub metadata_id: i64,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone)]
struct TitleArtist {
    title: Option<String>,
    artist: Option<String>,
}

impl Display for TitleArtist {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        let title = match self.title.as_ref() {
            Some(title) => title,
            None => "",
        };
        let _ = fmt.write_str(title);
        let _ = fmt.write_str(" - ");
        let artist = match self.artist.as_ref() {
            Some(artist) => artist,
            None => "",
        };
        let _ = fmt.write_str(artist);
        Ok(())
    }
}

impl PlayLog {
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
            // user_id.is_some() && guild_id.is_none()
            Self::get_last_played_by_user(conn, user_id.unwrap()).await
        }
    }

    pub async fn get_last_played_by_guild(
        conn: &PgPool,
        guild_id: i64,
    ) -> Result<Vec<String>, Error> {
        //let last_played: Vec<TitleArtist> = sqlx::query_as!(
        let mut last_played: Vec<TitleArtist> = Vec::new();
        let mut last_played_stream = sqlx::query_as!(
            TitleArtist,
            r#"
            select title, artist 
            from play_log 
            join metadata on 
            play_log.metadata_id = metadata.id 
            where guild_id = $1 order by created_at desc limit 5
            "#,
            guild_id
        )
        .fetch(conn);
        while let Some(item) = last_played_stream.next().await {
            // Process the item
            last_played.push(item?);
        }
        Ok(last_played.into_iter().map(|t| t.to_string()).collect())
    }

    pub async fn get_last_played_by_user(
        conn: &PgPool,
        user_id: i64,
    ) -> Result<Vec<String>, Error> {
        //let last_played: Vec<TitleArtist> = sqlx::query_as!(
        let mut last_played: Vec<TitleArtist> = Vec::new();
        let mut last_played_stream = sqlx::query_as!(
            TitleArtist,
            r#"
            select title, artist 
            from play_log 
            join metadata on 
            play_log.metadata_id = metadata.id 
            where user_id = $1 order by created_at desc limit 5
            "#,
            user_id
        )
        .fetch(conn);
        while let Some(item) = last_played_stream.next().await {
            // Process the item
            last_played.push(item?);
        }
        Ok(last_played.into_iter().map(|t| t.to_string()).collect())
    }
}
