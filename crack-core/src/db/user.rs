use crate::errors::CrackedError;
use crate::messaging::messages::TEST;
use ::chrono::Duration;
use sqlx::{
    types::chrono::{self},
    PgPool,
};

/// The base db struct for a discord user.
#[derive(Debug, Default)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub discriminator: Option<i16>,
    pub avatar_url: String,
    pub bot: bool,
    pub created_at: chrono::NaiveDate,
    pub updated_at: chrono::NaiveDate,
    pub last_seen: chrono::NaiveDate,
}

/// UserTrace is a struct that represents a trace of a user's activity.
pub struct UserTrace {
    pub user_id: i64,
    pub ts: chrono::NaiveDateTime,
    pub whence: Option<String>,
}

/// UserVote is a struct that represents a vote from a user for the bot on a toplist site.
#[derive(sqlx::FromRow)]
pub struct UserVote {
    pub id: i64,
    pub user_id: i64,
    pub site: String,
    pub timestamp: chrono::NaiveDateTime,
}

/// Implementations for the User struct.
impl User {
    /// Insert a test user into the database.
    pub async fn insert_test_user(pool: &PgPool, user_id: Option<i64>, username: Option<String>) {
        let user = username.unwrap_or(TEST.to_string());
        let user_id = user_id.unwrap_or(1);
        let result = sqlx::query!(
            r#"insert into public.user
            (id, username, avatar_url, bot, created_at, updated_at, last_seen) values ($1, $2, '', false, now(), now(), now())"#,
            user_id,
            user,
        )
        .execute(pool)
        .await;

        match result {
            Ok(_) => println!("User inserted successfully."),
            Err(e) => println!("Failed to insert user: {}", e),
        }
    }

    /// Get a user from the database by user_id.
    pub async fn get_user(pool: &PgPool, user_id: i64) -> Option<User> {
        let result = sqlx::query_as!(User, r#"SELECT * FROM public.user WHERE id = $1"#, user_id)
            .fetch_optional(pool)
            .await;

        match result {
            Ok(user) => Some(user?),
            Err(e) => {
                println!("Failed to get user: {}", e);
                None
            },
        }
    }

    /// Records the last seen time of a user.
    /// This probably should be brough out of the db part of the code.
    pub async fn record_last_seen(pool: &PgPool, user_id: i64) -> Result<UserTrace, sqlx::Error> {
        sqlx::query_as!(
            UserTrace,
            r#"INSERT INTO public.user_trace (user_id, ts, whence) VALUES ($1, now(), NULL) RETURNING user_id, ts, whence"#,
            user_id
        )
        .fetch_one(pool)
        .await
    }

    /// Insert a user into the database if it's new, update the username and lastseen otherwise.
    pub async fn insert_or_update_user(
        pool: &PgPool,
        user_id: i64,
        username: String,
    ) -> Result<User, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"INSERT INTO public.user (id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen)
            VALUES ($1, $2, 0, '', false, now(), now(), now())
            ON CONFLICT (id) DO UPDATE SET last_seen = now(), username = $2
            RETURNING id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen
            "#,
            user_id,
            username,
        )
        .fetch_one(pool)
        .await
    }
}

/// Implementations for the UserVote db actions.
impl UserVote {
    /// Insert a user vote into the database.
    pub async fn insert_user_vote(
        pool: &PgPool,
        user_id: i64,
        site: String,
    ) -> Result<UserVote, sqlx::Error> {
        sqlx::query_as!(
            UserVote,
            r#"INSERT INTO user_votes (user_id, site, timestamp)
            VALUES ($1, $2, now())
            RETURNING id, user_id, site, timestamp
            "#,
            user_id,
            site,
        )
        .fetch_one(pool)
        .await
    }

    /// Get the votes for a user.
    pub async fn get_user_votes(user_id: i64, pool: &PgPool) -> Result<Vec<UserVote>, sqlx::Error> {
        sqlx::query_as!(
            UserVote,
            r#"SELECT * FROM user_votes WHERE user_id = $1"#,
            user_id
        )
        .fetch_all(pool)
        .await
    }

    /// Check if the user has voted on a site in the last `duration`.
    pub async fn has_voted_recently(
        user_id: i64,
        site_name: String,
        duration: chrono::NaiveDateTime,
        pool: &PgPool,
    ) -> Result<bool, CrackedError> {
        sqlx::query_as!(
            UserVote,
            r#"SELECT * FROM user_votes WHERE user_id = $1 AND timestamp > $2 AND site = $3"#,
            user_id,
            duration,
            site_name
        )
        .fetch_optional(pool)
        .await
        .map(|vote| vote.is_some())
        .map_err(CrackedError::from)
    }

    /// Check if the user has voted on top.gg in the last 12 hours.
    pub async fn has_voted_recently_topgg(
        user_id: i64,
        pool: &PgPool,
    ) -> Result<bool, CrackedError> {
        let twelve_hours_ago = chrono::Utc::now().naive_utc() - Duration::hours(12);
        let site_name = "top.gg".to_string(); // Define the site you are checking for votes from

        Self::has_voted_recently(user_id, site_name, twelve_hours_ago, pool).await
    }
}

#[cfg(test)]
mod test {
    use super::UserVote;
    use super::TEST;
    use crate::db::User;
    use chrono::{Duration, Utc};
    use sqlx::PgPool;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    // /// Make sure the DATABASE_URL is set before running tests.
    #[ctor::ctor]
    fn set_env() {
        use std::env;

        env::set_var(
            "DATABASE_URL",
            "postgresql://postgres:mysecretpassword@localhost:5432/postgres",
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_user(pool: PgPool) {
        User::insert_test_user(&pool, Some(1), Some(TEST.to_string())).await;
        let user = User::get_user(&pool, 1).await.unwrap();
        assert_eq!(user.username, TEST);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_or_update_user(pool: PgPool) {
        User::insert_or_update_user(&pool, 1, TEST.to_string())
            .await
            .unwrap();
        let user = User::get_user(&pool, 1).await.unwrap();
        assert_eq!(user.username, TEST);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_insert_user_vote(pool: PgPool) {
        let insert_res = UserVote::insert_user_vote(&pool, 1, TEST.to_string()).await;
        assert!(insert_res.is_ok());
        let user_votes = UserVote::get_user_votes(1, &pool).await;
        assert!(user_votes.is_ok());
        let user_votes = user_votes.unwrap();
        assert_eq!(user_votes.len(), 1);
        let first = user_votes.first();
        assert!(first.is_some());
        assert_eq!(first.unwrap().site, TEST);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_has_voted_recently(pool: PgPool) {
        UserVote::insert_user_vote(&pool, 1, TEST.to_string())
            .await
            .unwrap();
        let has_voted = UserVote::has_voted_recently(
            1,
            TEST.to_string(),
            Utc::now()
                .naive_utc()
                .checked_add_signed(Duration::seconds(-5 * 60))
                .unwrap(),
            &pool,
        )
        .await
        .unwrap();
        assert_eq!(has_voted, true);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_has_voted_recently_topgg(pool: PgPool) {
        UserVote::insert_user_vote(&pool, 1, "top.gg".to_string())
            .await
            .unwrap();
        let has_voted = UserVote::has_voted_recently_topgg(1, &pool).await.unwrap();
        assert_eq!(has_voted, true);
    }
}
