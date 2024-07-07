use dbl::types::Webhook;
use lazy_static::lazy_static;
use std::env;
use warp::{body::BodyDeserializeError, http::StatusCode, path, reject, Filter, Rejection, Reply};

const WEBHOOK_SECRET_DEFAULT: &str = "my-that-secret";
const DATABASE_URL_DEFAULT: &str = "postgres://postgres:mysecretpassword@localhost/postgres";

lazy_static! {
    static ref WEBHOOK_SECRET: String =
        env::var("WEBHOOK_SECRET").unwrap_or(WEBHOOK_SECRET_DEFAULT.to_string());
    static ref DATABASE_URL: String =
        env::var("DATABASE_URL").unwrap_or(DATABASE_URL_DEFAULT.to_string());
}

/// Struct to hold the context for the voting server.
#[derive(Debug, Clone)]
pub struct VotingContext {
    pool: sqlx::PgPool,
    secret: String,
}

/// Implement the `VotingContext`.
impl VotingContext {
    async fn new() -> Self {
        let pool = sqlx::PgPool::connect(&DATABASE_URL)
            .await
            .expect("failed to connect to database");
        let secret = get_secret().to_string();
        VotingContext { pool, secret }
    }
}

/// NewClass for the Webhook to store in the database.
#[derive(Debug, serde::Deserialize, serde::Serialize, sqlx::FromRow, Clone, PartialEq, Eq)]
pub struct CrackedWebhook {
    webhook: Webhook,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Custom error type for unauthorized requests.
#[derive(Debug)]
struct Unauthorized;

impl warp::reject::Reject for Unauthorized {}

impl std::fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Unauthorized")
    }
}

impl std::error::Error for Unauthorized {}

/// Custom error type for unauthorized requests.
#[derive(Debug)]
struct Sqlx(sqlx::Error);

impl warp::reject::Reject for Sqlx {}

impl std::fmt::Display for Sqlx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl std::error::Error for Sqlx {}
/// Get the webhook secret from the environment.
fn get_secret() -> &'static str {
    &WEBHOOK_SECRET
}

/// Write the received webhook to the database.
async fn write_webhook_to_db(
    ctx: &'static VotingContext,
    webhook: Webhook,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO vote_webhook
            (bot_id, user_id, kind, is_weekend, query, created_at)
        VALUES
            ($1, $2, $3, $4, $5, now())
        "#,
        webhook.bot.0 as i64,
        webhook.user.0 as i64,
        webhook.kind as i16,
        webhook.is_weekend,
        webhook.query,
    )
    .execute(&ctx.pool)
    .await?;
    Ok(())
}

/// Create a filter that checks the `Authorization` header against the secret.
fn header(secret: &str) -> impl Filter<Extract = (), Error = Rejection> + Clone + '_ {
    warp::header::<String>("authorization")
        .and_then(move |val: String| async move {
            if val == secret {
                println!("Authorized");
                Ok(())
            } else {
                println!("Not Authorized");
                Err(reject::custom(Unauthorized))
            }
        })
        .untuple_one()
}

/// Async function to process the received webhook.
async fn process_webhook(
    ctx: &'static VotingContext,
    hook: Webhook,
) -> Result<impl Reply, Rejection> {
    write_webhook_to_db(ctx, hook.clone()).await.map_err(Sqlx)?;
    println!("{:?}", hook);
    Ok(warp::reply())
}
/// Create a filter that handles the webhook.
async fn get_webhook(
    ctx: &'static VotingContext,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    println!("get_webhook");

    warp::post()
        .and(path!("dbl" / "webhook"))
        .and(header(&ctx.secret))
        .and(warp::body::json())
        .and_then(move |hook: Webhook| async move { process_webhook(ctx, hook).await })
        .recover(custom_error)
}

/// Run the server.
pub async fn run() -> &'static VotingContext {
    let ctx = Box::leak(Box::new(VotingContext::new().await));
    warp::serve(get_webhook(ctx).await)
        .run(([127, 0, 0, 1], 3030))
        .await;
    ctx
}

/// Custom error handling for the server.
async fn custom_error(err: Rejection) -> Result<impl Reply, Rejection> {
    if err.find::<BodyDeserializeError>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply(),
            StatusCode::BAD_REQUEST,
        ))
    } else if err.find::<Unauthorized>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply(),
            StatusCode::UNAUTHORIZED,
        ))
    } else {
        Err(err)
    }
}

#[cfg(test)]
mod test {
    use super::{get_secret, get_webhook};
    use super::{StatusCode, VotingContext, Webhook};

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    //#[sqlx::test(migrator = "MIGRATOR")]
    #[sqlx::test]
    async fn test_bad_req() {
        let ctx = Box::leak(Box::new(VotingContext::new().await));
        let res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .reply(&get_webhook(ctx).await)
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    //#[sqlx::test(migrator = "MIGRATOR")]
    #[sqlx::test]
    async fn test_authorized() {
        let ctx = Box::leak(Box::new(VotingContext::new().await));
        let res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .header("authorization", get_secret())
            .json(&Webhook {
                bot: dbl::types::BotId(1),
                user: dbl::types::UserId(3),
                kind: dbl::types::WebhookType::Test,
                is_weekend: false,
                query: Some("test".to_string()),
            })
            .reply(&get_webhook(ctx).await)
            .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}
