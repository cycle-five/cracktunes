//! Writes the queue audit log. See `music::audit`.

use crate::music::audit::AuditEvent;
use sqlx::{postgres::PgPool, types::Json};
use tokio::sync::mpsc;

/// Room for bursts, such as a playlist add while a clear runs. `emit` drops
/// rather than waits when this is full.
pub const AUDIT_CHANNEL_CAPACITY: usize = 1024;

/// Insert one event.
pub async fn insert_audit_event(pool: &PgPool, e: &AuditEvent) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO queue_audit
             (at, guild_id, voice_channel_id, origin_channel_id, actor_user_id,
              source, command, action, detail)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        e.at,
        e.guild_id.get() as i64,
        e.voice_channel.map(|c| c.get() as i64),
        e.actor.origin_channel().map(|c| c.get() as i64),
        e.actor.user().map(|u| u.get() as i64),
        e.actor.source().as_str(),
        e.actor.command(),
        e.action.name(),
        Json(&e.action) as _,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Start the writer and return its sender. An insert that fails is logged and
/// the writer carries on: the log is best-effort, never a reason to stop.
pub fn spawn_audit_writer(pool: PgPool) -> mpsc::Sender<AuditEvent> {
    let (tx, mut rx) = mpsc::channel::<AuditEvent>(AUDIT_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            if let Err(err) = insert_audit_event(&pool, &e).await {
                tracing::warn!("queue audit: insert failed in {}: {err}", e.guild_id);
            }
        }
    });
    tx
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::audit::{Action, Actor, AuditEvent, TrackRef};
    use serenity::all::{GenericChannelId, GuildId, UserId};

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn an_event_round_trips_through_the_table(pool: PgPool) {
        let action = Action::Move {
            track: TrackRef {
                title: Some("t".into()),
                url: Some("https://x".into()),
            },
            from: 3,
            to: 1,
        };
        let e = AuditEvent {
            at: chrono::Utc::now(),
            guild_id: GuildId::new(11),
            voice_channel: None,
            actor: Actor::for_command(
                UserId::new(22),
                false,
                "movesong",
                Some(GenericChannelId::new(33)),
            ),
            action: action.clone(),
        };
        insert_audit_event(&pool, &e).await.unwrap();
        let row = sqlx::query!(
            r#"SELECT guild_id, actor_user_id, origin_channel_id, source, command, action,
                      detail AS "detail!: sqlx::types::Json<Action>"
               FROM queue_audit"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (row.guild_id, row.actor_user_id, row.origin_channel_id),
            (11, Some(22), Some(33))
        );
        assert_eq!(
            (
                row.source.as_str(),
                row.command.as_str(),
                row.action.as_str()
            ),
            ("slash", "movesong", "move")
        );
        assert_eq!(row.detail.0, action);
    }
}
