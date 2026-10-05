//! Writes the queue audit log. See `music::audit`.

use crate::music::audit::{Action, AuditEvent};
use crate::music::audit_view::AuditRow;
use chrono::{DateTime, Utc};
use serenity::all::{GuildId, UserId};
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

/// What `/auditlog` asked for. Every field is optional.
#[derive(Debug, Default, Clone)]
pub struct AuditFilter<'a> {
    pub user: Option<UserId>,
    pub action: Option<&'a str>,
    pub source: Option<&'a str>,
    pub since: Option<DateTime<Utc>>,
}

/// A guild's audit rows, newest first, at most `limit`.
pub async fn recent_audit(
    pool: &PgPool,
    guild_id: GuildId,
    f: &AuditFilter<'_>,
    limit: i64,
) -> sqlx::Result<Vec<AuditRow>> {
    let rows = sqlx::query!(
        r#"SELECT at, actor_user_id, source, command, action,
                  detail AS "detail!: Json<Action>"
           FROM queue_audit
           WHERE guild_id = $1
             AND ($2::bigint      IS NULL OR actor_user_id = $2)
             AND ($3::text        IS NULL OR action = $3)
             AND ($4::text        IS NULL OR source = $4)
             AND ($5::timestamptz IS NULL OR at >= $5)
           ORDER BY at DESC
           LIMIT $6"#,
        guild_id.get() as i64,
        f.user.map(|u| u.get() as i64),
        f.action,
        f.source,
        f.since,
        limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditRow {
            at: r.at,
            actor_user_id: r.actor_user_id,
            source: r.source,
            command: r.command,
            action: r.action,
            detail: r.detail.0,
        })
        .collect())
}

/// Where a history page starts. Ids are insertion order: the audit writer is
/// one FIFO task, so "after id X" can never miss a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditCursor {
    /// The newest rows.
    #[default]
    Newest,
    /// Rows older than this id.
    Before(i64),
    /// Rows newer than this id: the oldest of them first in line, so a burst
    /// bigger than one page is read across polls, never skipped.
    After(i64),
}

/// A row with its id, for paging.
#[derive(Debug, Clone)]
pub struct AuditPageRow {
    pub id: i64,
    pub row: AuditRow,
}

/// A page of a guild's audit rows, newest first: see [`AuditCursor`] for
/// which `limit` rows. With `hide_gp_since`, the rows a `/gp` game started then
/// hides are left out in the query (the rule is
/// [`crate::music::audit_view::hidden_by_game`]), so every row returned is one
/// the page shows and its id is a cursor the page can page by.
pub async fn audit_page(
    pool: &PgPool,
    guild_id: GuildId,
    f: &AuditFilter<'_>,
    cursor: AuditCursor,
    limit: i64,
    hide_gp_since: Option<DateTime<Utc>>,
) -> sqlx::Result<Vec<AuditPageRow>> {
    let guild = guild_id.get() as i64;
    let user = f.user.map(|u| u.get() as i64);
    if let AuditCursor::After(after) = cursor {
        let rows = sqlx::query!(
            r#"SELECT id, at, actor_user_id, source, command, action,
                      detail AS "detail!: Json<Action>"
               FROM queue_audit
               WHERE guild_id = $1
                 AND ($2::bigint      IS NULL OR actor_user_id = $2)
                 AND ($3::text        IS NULL OR action = $3)
                 AND ($4::text        IS NULL OR source = $4)
                 AND ($5::timestamptz IS NULL OR at >= $5)
                 AND id > $6
                 AND NOT ($8::timestamptz IS NOT NULL
                          AND (command = 'gp' OR command LIKE 'gp %')
                          AND at >= $8)
               ORDER BY id ASC
               LIMIT $7"#,
            guild,
            user,
            f.action,
            f.source,
            f.since,
            after,
            limit,
            hide_gp_since,
        )
        .fetch_all(pool)
        .await?;
        let mut out: Vec<AuditPageRow> = rows
            .into_iter()
            .map(|r| AuditPageRow {
                id: r.id,
                row: AuditRow {
                    at: r.at,
                    actor_user_id: r.actor_user_id,
                    source: r.source,
                    command: r.command,
                    action: r.action,
                    detail: r.detail.0,
                },
            })
            .collect();
        out.reverse();
        return Ok(out);
    }
    let before = match cursor {
        AuditCursor::Before(id) => Some(id),
        _ => None,
    };
    let rows = sqlx::query!(
        r#"SELECT id, at, actor_user_id, source, command, action,
                  detail AS "detail!: Json<Action>"
           FROM queue_audit
           WHERE guild_id = $1
             AND ($2::bigint      IS NULL OR actor_user_id = $2)
             AND ($3::text        IS NULL OR action = $3)
             AND ($4::text        IS NULL OR source = $4)
             AND ($5::timestamptz IS NULL OR at >= $5)
             AND ($6::bigint      IS NULL OR id < $6)
             AND NOT ($8::timestamptz IS NOT NULL
                      AND (command = 'gp' OR command LIKE 'gp %')
                      AND at >= $8)
           ORDER BY id DESC
           LIMIT $7"#,
        guild,
        user,
        f.action,
        f.source,
        f.since,
        before,
        limit,
        hide_gp_since,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditPageRow {
            id: r.id,
            row: AuditRow {
                at: r.at,
                actor_user_id: r.actor_user_id,
                source: r.source,
                command: r.command,
                action: r.action,
                detail: r.detail.0,
            },
        })
        .collect())
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
    use crate::music::audit::{Actor, TrackRef};
    use serenity::all::GenericChannelId;

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

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn filters_and_guild_isolation(pool: PgPool) {
        use crate::music::audit::BotReason;
        let g = GuildId::new(1);
        let other = GuildId::new(2);
        let now = chrono::Utc::now();
        let ev = |guild, mins_ago: i64, actor: Actor, action: Action| AuditEvent {
            at: now - chrono::Duration::minutes(mins_ago),
            guild_id: guild,
            voice_channel: None,
            actor,
            action,
        };
        let alice = Actor::for_command(UserId::new(10), false, "pause", None);
        let bob = Actor::web(UserId::new(20), "move");
        for e in [
            ev(g, 1, alice.clone(), Action::Pause),
            ev(
                g,
                30,
                bob,
                Action::Move {
                    track: TrackRef {
                        title: Some("m".into()),
                        url: None,
                    },
                    from: 2,
                    to: 0,
                },
            ),
            ev(
                g,
                600,
                Actor::bot(BotReason::IdleTimeout),
                Action::Leave { discarded: 3 },
            ),
            ev(other, 1, alice, Action::Pause),
        ] {
            insert_audit_event(&pool, &e).await.unwrap();
        }

        let all = recent_audit(&pool, g, &AuditFilter::default(), 10)
            .await
            .unwrap();
        assert_eq!(
            all.iter().map(|r| r.action.as_str()).collect::<Vec<_>>(),
            vec!["pause", "move", "leave"],
            "newest first, guild 1 only"
        );

        let by_user = recent_audit(
            &pool,
            g,
            &AuditFilter {
                user: Some(UserId::new(20)),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_user.len(), 1);
        assert_eq!(by_user[0].source, "web");

        let by_action = recent_audit(
            &pool,
            g,
            &AuditFilter {
                action: Some("leave"),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();
        assert_eq!((by_action.len(), by_action[0].actor_user_id), (1, None));

        let by_source = recent_audit(
            &pool,
            g,
            &AuditFilter {
                source: Some("slash"),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();
        assert_eq!(by_source.len(), 1);

        let recent = recent_audit(
            &pool,
            g,
            &AuditFilter {
                since: Some(now - chrono::Duration::hours(1)),
                ..Default::default()
            },
            10,
        )
        .await
        .unwrap();
        assert_eq!(recent.len(), 2);

        let capped = recent_audit(&pool, g, &AuditFilter::default(), 2)
            .await
            .unwrap();
        assert_eq!(capped.len(), 2);
        assert_eq!(capped[0].detail, Action::Pause);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn audit_page_pages_by_id_both_ways(pool: PgPool) {
        let g = GuildId::new(1);
        let other = GuildId::new(2);
        let now = chrono::Utc::now();
        let actor = Actor::web(UserId::new(20), "move");
        // Ten rows in guild 1, inserted in order; one in guild 2 between them.
        for i in 0..10 {
            let e = AuditEvent {
                at: now,
                guild_id: g,
                voice_channel: None,
                actor: actor.clone(),
                action: Action::Shuffle { count: i },
            };
            insert_audit_event(&pool, &e).await.unwrap();
            if i == 4 {
                let e = AuditEvent {
                    guild_id: other,
                    ..e
                };
                insert_audit_event(&pool, &e).await.unwrap();
            }
        }
        let count_of = |rows: &[AuditPageRow]| -> Vec<usize> {
            rows.iter()
                .map(|r| match r.row.detail {
                    Action::Shuffle { count } => count,
                    _ => unreachable!(),
                })
                .collect()
        };
        let all = AuditFilter::default();

        let newest = audit_page(&pool, g, &all, AuditCursor::Newest, 3, None)
            .await
            .unwrap();
        assert_eq!(
            count_of(&newest),
            vec![9, 8, 7],
            "newest first, guild 1 only"
        );
        assert!(newest.windows(2).all(|w| w[0].id > w[1].id));

        let before = audit_page(&pool, g, &all, AuditCursor::Before(newest[2].id), 3, None)
            .await
            .unwrap();
        assert_eq!(count_of(&before), vec![6, 5, 4]);

        // After the 3rd-oldest row: the OLDEST three above it, newest first.
        let oldest = audit_page(&pool, g, &all, AuditCursor::Newest, 10, None)
            .await
            .unwrap();
        let third_oldest = oldest[7].id;
        let after = audit_page(&pool, g, &all, AuditCursor::After(third_oldest), 3, None)
            .await
            .unwrap();
        assert_eq!(
            count_of(&after),
            vec![5, 4, 3],
            "the window right above the cursor"
        );

        let nothing_newer = audit_page(&pool, g, &all, AuditCursor::After(newest[0].id), 3, None)
            .await
            .unwrap();
        assert!(nothing_newer.is_empty());

        // Filters apply with a cursor too.
        let by_action = AuditFilter {
            action: Some("pause"),
            ..AuditFilter::default()
        };
        assert!(
            audit_page(&pool, g, &by_action, AuditCursor::Newest, 10, None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn audit_page_hides_a_running_games_rows_as_hidden_by_game_does(pool: PgPool) {
        use crate::music::audit_view::hidden_by_game;
        let g = GuildId::new(1);
        // Whole seconds, as a game's start is: Postgres keeps microseconds, so
        // a nanosecond start would compare differently in SQL and in Rust.
        let start = chrono::DateTime::from_timestamp(chrono::Utc::now().timestamp(), 0).unwrap();
        let secs = chrono::Duration::seconds;
        // (command, at): hidden are gp commands at or after the start.
        let rows = [
            ("gp", start - secs(60)), // before the game: kept
            ("play", start + secs(1)),
            ("gp", start),                // hidden
            ("gp skip", start + secs(2)), // hidden
            ("gpx", start + secs(3)),     // not a gp command: kept
            ("gp", start + secs(4)),      // hidden
            ("skip", start + secs(5)),
        ];
        for (i, (command, at)) in rows.into_iter().enumerate() {
            let e = AuditEvent {
                at,
                guild_id: g,
                voice_channel: None,
                actor: Actor::for_command(UserId::new(20), false, command, None),
                action: Action::Shuffle { count: i },
            };
            insert_audit_event(&pool, &e).await.unwrap();
        }
        let all = AuditFilter::default();
        let ids = |rows: &[AuditPageRow]| rows.iter().map(|r| r.id).collect::<Vec<_>>();
        let unhidden = audit_page(&pool, g, &all, AuditCursor::Newest, 50, None)
            .await
            .unwrap();
        let want: Vec<i64> = unhidden
            .iter()
            .filter(|r| !hidden_by_game(&r.row, Some(start)))
            .map(|r| r.id)
            .collect();
        assert_eq!((unhidden.len(), want.len()), (7, 4), "three rows to hide");

        let newest = audit_page(&pool, g, &all, AuditCursor::Newest, 50, Some(start))
            .await
            .unwrap();
        assert_eq!(ids(&newest), want, "newest");

        let oldest = *ids(&unhidden).last().unwrap();
        let after = audit_page(&pool, g, &all, AuditCursor::After(oldest), 50, Some(start))
            .await
            .unwrap();
        assert_eq!(ids(&after), want[..want.len() - 1], "after the oldest");

        // A page of two comes back full: the hidden rows do not use up the limit.
        let two = audit_page(&pool, g, &all, AuditCursor::Newest, 2, Some(start))
            .await
            .unwrap();
        assert_eq!(ids(&two), want[..2]);
        let next = audit_page(
            &pool,
            g,
            &all,
            AuditCursor::Before(two[1].id),
            2,
            Some(start),
        )
        .await
        .unwrap();
        assert_eq!(ids(&next), want[2..]);
    }
}
