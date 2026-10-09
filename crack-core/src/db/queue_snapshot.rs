//! A guild's music queue as rows, written at shutdown and claimed on the way
//! back up (#595). See `music::resume`.

use serde::{Deserialize, Serialize};
use sqlx::{types::Json, PgPool};

/// One queued track, as much as a rebuild needs: what `build_track` plays and
/// the queue shows, and who asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotTrack {
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_secs: Option<i64>,
    /// Who asked for it. Autoplay's picks are recorded as user 1 (`Some(1)`);
    /// `None` means the requester could not be read.
    pub requester: Option<i64>,
    /// Artwork link; rows written before this field existed read as `None`.
    #[serde(default)]
    pub thumbnail: Option<String>,
}

/// A guild's queue at shutdown. Tracks are in play order, current first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub guild_id: i64,
    pub voice_channel_id: i64,
    pub text_channel_id: Option<i64>,
    pub status_channel_id: Option<i64>,
    pub status_message_id: Option<i64>,
    /// Of the current track.
    pub position_ms: i64,
    pub paused: bool,
    pub looping: bool,
    pub autoplay: bool,
    pub tracks: Vec<SnapshotTrack>,
}

/// A claimed snapshot and how long ago it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSnapshot {
    pub snapshot: QueueSnapshot,
    pub age_secs: i64,
}

/// Write every snapshot in one transaction. A guild already saved is replaced,
/// its clock restarted.
pub async fn save_all(pool: &PgPool, snapshots: &[QueueSnapshot]) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    for s in snapshots {
        sqlx::query!(
            r#"INSERT INTO queue_snapshot (
                   guild_id, voice_channel_id, text_channel_id, status_channel_id,
                   status_message_id, position_ms, paused, looping, autoplay, tracks
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (guild_id) DO UPDATE SET
                   saved_at = now(),
                   voice_channel_id = EXCLUDED.voice_channel_id,
                   text_channel_id = EXCLUDED.text_channel_id,
                   status_channel_id = EXCLUDED.status_channel_id,
                   status_message_id = EXCLUDED.status_message_id,
                   position_ms = EXCLUDED.position_ms,
                   paused = EXCLUDED.paused,
                   looping = EXCLUDED.looping,
                   autoplay = EXCLUDED.autoplay,
                   tracks = EXCLUDED.tracks"#,
            s.guild_id,
            s.voice_channel_id,
            s.text_channel_id,
            s.status_channel_id,
            s.status_message_id,
            s.position_ms,
            s.paused,
            s.looping,
            s.autoplay,
            Json(&s.tracks) as _,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Take the guild's snapshot out of the table, if it has one. The delete is
/// the claim: a second call -- the next gateway reconnect -- finds nothing.
pub async fn claim(pool: &PgPool, guild_id: i64) -> sqlx::Result<Option<ClaimedSnapshot>> {
    let row = sqlx::query!(
        r#"DELETE FROM queue_snapshot WHERE guild_id = $1
           RETURNING guild_id, voice_channel_id, text_channel_id, status_channel_id,
                     status_message_id, position_ms, paused, looping, autoplay,
                     tracks AS "tracks!: Json<Vec<SnapshotTrack>>",
                     EXTRACT(EPOCH FROM now() - saved_at)::BIGINT AS "age_secs!""#,
        guild_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ClaimedSnapshot {
        snapshot: QueueSnapshot {
            guild_id: r.guild_id,
            voice_channel_id: r.voice_channel_id,
            text_channel_id: r.text_channel_id,
            status_channel_id: r.status_channel_id,
            status_message_id: r.status_message_id,
            position_ms: r.position_ms,
            paused: r.paused,
            looping: r.looping,
            autoplay: r.autoplay,
            tracks: r.tracks.0,
        },
        age_secs: r.age_secs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn track(n: i64, requester: Option<i64>) -> SnapshotTrack {
        SnapshotTrack {
            url: format!("https://www.youtube.com/watch?v=t{n}"),
            title: Some(format!("song {n}")),
            artist: None,
            duration_secs: Some(200 + n),
            requester,
            thumbnail: None,
        }
    }

    fn snapshot(guild_id: i64) -> QueueSnapshot {
        QueueSnapshot {
            guild_id,
            voice_channel_id: 10,
            text_channel_id: Some(20),
            status_channel_id: Some(20),
            status_message_id: Some(500),
            position_ms: 61_500,
            paused: true,
            looping: false,
            autoplay: true,
            tracks: vec![track(0, Some(100)), track(1, None), track(2, Some(300))],
        }
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_saved_snapshot_comes_back_whole_and_in_order(pool: PgPool) -> sqlx::Result<()> {
        save_all(&pool, &[snapshot(1), snapshot(2)]).await?;
        let got = claim(&pool, 1).await?.expect("saved");
        assert_eq!(got.snapshot, snapshot(1));
        assert!(got.age_secs < 5);
        Ok(())
    }

    /// Review Focus 1: `on_guild_create` runs again on every reconnect.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_snapshot_is_claimed_once(pool: PgPool) -> sqlx::Result<()> {
        save_all(&pool, &[snapshot(1)]).await?;
        assert!(claim(&pool, 1).await?.is_some());
        assert!(claim(&pool, 1).await?.is_none());
        assert!(claim(&pool, 99).await?.is_none(), "never saved");
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_second_save_replaces_the_first_and_restarts_the_clock(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        save_all(&pool, &[snapshot(1)]).await?;
        sqlx::query("UPDATE queue_snapshot SET saved_at = now() - interval '400 seconds'")
            .execute(&pool)
            .await?;
        let mut newer = snapshot(1);
        newer.tracks.truncate(1);
        newer.paused = false;
        save_all(&pool, &[newer.clone()]).await?;
        let got = claim(&pool, 1).await?.expect("saved");
        assert_eq!(got.snapshot, newer);
        assert!(got.age_secs < 5, "saved_at was reset");
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_claim_reports_how_long_ago_it_was_saved(pool: PgPool) -> sqlx::Result<()> {
        save_all(&pool, &[snapshot(1)]).await?;
        sqlx::query("UPDATE queue_snapshot SET saved_at = now() - interval '400 seconds'")
            .execute(&pool)
            .await?;
        let got = claim(&pool, 1).await?.expect("saved");
        assert!((400..410).contains(&got.age_secs), "{}", got.age_secs);
        Ok(())
    }

    #[test]
    fn a_track_round_trips_through_json() {
        let t = track(3, Some(7));
        let back: SnapshotTrack =
            serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }
}
