use dbl::types::Webhook;
use lazy_static::lazy_static;
use std::env;
use warp::{body::BodyDeserializeError, http::StatusCode, path, reject, Filter, Rejection, Reply};

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
struct SQLX(sqlx::Error);

impl warp::reject::Reject for SQLX {}

impl std::fmt::Display for SQLX {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl std::error::Error for SQLX {}

lazy_static! {
    static ref WEBHOOK_SECRET: String =
        env::var("WEBHOOK_SECRET").unwrap_or("missing secret".to_string());
}

/// Get the webhook secret from the environment.
fn get_secret() -> &'static str {
    &WEBHOOK_SECRET
}

///
async fn write_webhook_to_db(pool: sqlx::PgPool, webhook: Webhook) -> Result<(), sqlx::Error> {
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
    .execute(&pool)
    .await?;
    Ok(())
}

/// Create a filter that checks the `Authorization` header against the secret.
fn header(secret: &'static str) -> impl Filter<Extract = (), Error = Rejection> + Clone {
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

async fn process_webhook(hook: Webhook) -> Result<impl Reply, Rejection> {
    let pool = sqlx::PgPool::connect(&env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    write_webhook_to_db(pool, hook.clone())
        .await
        .map_err(SQLX)?;
    println!("{:?}", hook);
    Ok(warp::reply())
}
/// Create a filter that handles the webhook.
async fn get_webhook() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let secret = get_secret();
    println!("get_webhook");

    warp::post()
        .and(path!("dbl" / "webhook"))
        .and(header(secret))
        .and(warp::body::json())
        .and_then(move |hook: Webhook| async move { process_webhook(hook).await })
        .recover(custom_error)
}

/// Run the server.
pub async fn run() {
    warp::serve(get_webhook().await)
        .run(([127, 0, 0, 1], 3030))
        .await;
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
    use super::*;
    use warp::http::StatusCode;

    #[tokio::test]
    async fn test_bad_req() {
        let res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .reply(&get_webhook().await)
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_authorized() {
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
            .reply(&get_webhook().await)
            .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}
