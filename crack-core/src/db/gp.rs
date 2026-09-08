//! Rows for a `/gp` game -- see `migrations/20260908120000_gp_game.sql`.
//!
//! [`GpSaved`] is a whole game as rows: the game, its players, its rounds and
//! every song with its guesses, likes and votes. [`GpSaved::save`] writes all
//! of it in one transaction and is idempotent, so the caller can hand it the
//! game as it stands at any moment and the tables end up describing exactly
//! that. It is written a handful of statements at a time whatever the size of
//! the game, because every table is loaded through `UNNEST` in one statement
//! rather than a row at a time.
//!
//! Nothing here knows about the in-memory game; `commands::music::gp_persist`
//! converts in both directions. Keeping the persisted shape its own set of
//! plain types is what lets a later write-behind send the same rows.

use sqlx::{PgPool, Postgres, Transaction};

/// How a game left the live set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpOutcome {
    /// Every round played.
    Finished,
    /// `/gp end`.
    Ended,
    /// Torn down for any other reason: a message that could not be posted, the
    /// bot removed from voice.
    Abandoned,
    /// Was live when the bot went down and was not resumed: too long an outage,
    /// or nobody left in the voice channel.
    Lost,
}

impl GpOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Ended => "ended",
            Self::Abandoned => "abandoned",
            Self::Lost => "lost",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpGameRow {
    pub guild_id: i64,
    /// Unix seconds of `/gp start`; with `guild_id`, the game's identity.
    pub started_at: i64,
    pub host_id: i64,
    pub voice_channel_id: i64,
    pub text_channel_id: i64,
    pub category: String,
    /// `submitting` | `playing` | `finished`
    pub phase: String,
    pub current_round: i32,
    pub current_track: i32,
    pub timer_secs: i64,
    pub clip_start_secs: Option<i64>,
    pub clip_length_secs: Option<i64>,
    pub generation: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpPlayerRow {
    pub user_id: i64,
    pub display_name: String,
    pub score: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpRoundRow {
    pub round_idx: i32,
    pub prompt: String,
    pub closes_at: Option<i64>,
    pub prompt_channel_id: Option<i64>,
    pub prompt_message_id: Option<i64>,
}

/// A song in a round, with everything the room did to it. `position` is `None`
/// while it is still a submission in an open window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpTrackRow {
    pub round_idx: i32,
    pub submitter_id: i64,
    pub position: Option<i32>,
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_secs: Option<i64>,
    pub play_full: bool,
    pub message_channel_id: Option<i64>,
    pub message_id: Option<i64>,
    /// (guesser, guessed submitter)
    pub guesses: Vec<(i64, i64)>,
    pub likes: Vec<i64>,
    pub skip_votes: Vec<i64>,
    pub full_votes: Vec<i64>,
}

/// A whole game as rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpSaved {
    pub game: GpGameRow,
    pub players: Vec<GpPlayerRow>,
    pub rounds: Vec<GpRoundRow>,
    pub tracks: Vec<GpTrackRow>,
}

/// A live game as loaded, with how long ago the bot last saw it.
#[derive(Clone, Debug)]
pub struct GpLoaded {
    pub saved: GpSaved,
    pub last_seen_secs_ago: i64,
}

const VOTE_SKIP: &str = "skip";
const VOTE_FULL: &str = "full";

impl GpSaved {
    /// Write the game as it stands. Returns the `gp_game.id`.
    ///
    /// Any *other* live row for the guild is closed as lost first: two games
    /// cannot run in one guild, so it can only be a leftover from a game that
    /// died before it was written down as over.
    pub async fn save(&self, pool: &PgPool) -> sqlx::Result<i64> {
        let mut tx = pool.begin().await?;
        let g = &self.game;

        sqlx::query!(
            r#"UPDATE gp_game
               SET finished_at = now(), outcome = 'lost'
               WHERE guild_id = $1 AND finished_at IS NULL AND started_at <> $2"#,
            g.guild_id,
            g.started_at,
        )
        .execute(&mut *tx)
        .await?;

        let id = sqlx::query_scalar!(
            r#"INSERT INTO gp_game (
                   guild_id, started_at, host_id, voice_channel_id, text_channel_id,
                   category, phase, current_round, current_track, timer_secs,
                   clip_start_secs, clip_length_secs, generation, last_seen_at
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now())
               ON CONFLICT (guild_id, started_at) DO UPDATE SET
                   phase = EXCLUDED.phase,
                   current_round = EXCLUDED.current_round,
                   current_track = EXCLUDED.current_track,
                   generation = EXCLUDED.generation,
                   text_channel_id = EXCLUDED.text_channel_id,
                   last_seen_at = now()
               RETURNING id"#,
            g.guild_id,
            g.started_at,
            g.host_id,
            g.voice_channel_id,
            g.text_channel_id,
            g.category,
            g.phase,
            g.current_round,
            g.current_track,
            g.timer_secs,
            g.clip_start_secs,
            g.clip_length_secs,
            g.generation,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.save_players(&mut tx, id).await?;
        self.save_rounds(&mut tx, id).await?;
        self.save_tracks(&mut tx, id).await?;
        self.save_reactions(&mut tx, id).await?;

        if g.phase == "finished" {
            mark_finished_in(&mut tx, g.guild_id, g.started_at, GpOutcome::Finished).await?;
        }
        tx.commit().await?;
        Ok(id)
    }

    async fn save_players(&self, tx: &mut Transaction<'_, Postgres>, id: i64) -> sqlx::Result<()> {
        if self.players.is_empty() {
            return Ok(());
        }
        let user_ids: Vec<i64> = self.players.iter().map(|p| p.user_id).collect();
        let names: Vec<String> = self
            .players
            .iter()
            .map(|p| p.display_name.clone())
            .collect();
        let scores: Vec<i32> = self.players.iter().map(|p| p.score).collect();
        sqlx::query!(
            r#"INSERT INTO gp_player (game_id, user_id, display_name, score)
               SELECT $1, * FROM UNNEST($2::bigint[], $3::text[], $4::int[])
               ON CONFLICT (game_id, user_id) DO UPDATE SET
                   display_name = EXCLUDED.display_name,
                   score = EXCLUDED.score"#,
            id,
            &user_ids,
            &names,
            &scores,
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn save_rounds(&self, tx: &mut Transaction<'_, Postgres>, id: i64) -> sqlx::Result<()> {
        if self.rounds.is_empty() {
            return Ok(());
        }
        let idxs: Vec<i32> = self.rounds.iter().map(|r| r.round_idx).collect();
        let prompts: Vec<String> = self.rounds.iter().map(|r| r.prompt.clone()).collect();
        let closes: Vec<Option<i64>> = self.rounds.iter().map(|r| r.closes_at).collect();
        let chans: Vec<Option<i64>> = self.rounds.iter().map(|r| r.prompt_channel_id).collect();
        let msgs: Vec<Option<i64>> = self.rounds.iter().map(|r| r.prompt_message_id).collect();
        sqlx::query!(
            r#"INSERT INTO gp_round (game_id, round_idx, prompt, closes_at, prompt_channel_id, prompt_message_id)
               SELECT $1, * FROM UNNEST($2::int[], $3::text[], $4::bigint[], $5::bigint[], $6::bigint[])
               ON CONFLICT (game_id, round_idx) DO UPDATE SET
                   closes_at = EXCLUDED.closes_at,
                   prompt_channel_id = EXCLUDED.prompt_channel_id,
                   prompt_message_id = EXCLUDED.prompt_message_id"#,
            id,
            &idxs,
            &prompts,
            &closes as &[Option<i64>],
            &chans as &[Option<i64>],
            &msgs as &[Option<i64>],
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn save_tracks(&self, tx: &mut Transaction<'_, Postgres>, id: i64) -> sqlx::Result<()> {
        if self.tracks.is_empty() {
            return Ok(());
        }
        let t = &self.tracks;
        let round_idxs: Vec<i32> = t.iter().map(|x| x.round_idx).collect();
        let submitters: Vec<i64> = t.iter().map(|x| x.submitter_id).collect();
        let positions: Vec<Option<i32>> = t.iter().map(|x| x.position).collect();
        let urls: Vec<String> = t.iter().map(|x| x.url.clone()).collect();
        let titles: Vec<Option<String>> = t.iter().map(|x| x.title.clone()).collect();
        let artists: Vec<Option<String>> = t.iter().map(|x| x.artist.clone()).collect();
        let durations: Vec<Option<i64>> = t.iter().map(|x| x.duration_secs).collect();
        let fulls: Vec<bool> = t.iter().map(|x| x.play_full).collect();
        let chans: Vec<Option<i64>> = t.iter().map(|x| x.message_channel_id).collect();
        let msgs: Vec<Option<i64>> = t.iter().map(|x| x.message_id).collect();
        sqlx::query!(
            r#"INSERT INTO gp_track (
                   game_id, round_idx, submitter_id, position, url, title, artist,
                   duration_secs, play_full, message_channel_id, message_id
               )
               SELECT $1, * FROM UNNEST(
                   $2::int[], $3::bigint[], $4::int[], $5::text[], $6::text[], $7::text[],
                   $8::bigint[], $9::bool[], $10::bigint[], $11::bigint[]
               )
               ON CONFLICT (game_id, round_idx, submitter_id) DO UPDATE SET
                   position = EXCLUDED.position,
                   url = EXCLUDED.url,
                   title = EXCLUDED.title,
                   artist = EXCLUDED.artist,
                   duration_secs = EXCLUDED.duration_secs,
                   play_full = EXCLUDED.play_full,
                   message_channel_id = EXCLUDED.message_channel_id,
                   message_id = EXCLUDED.message_id"#,
            id,
            &round_idxs,
            &submitters,
            &positions as &[Option<i32>],
            &urls,
            &titles as &[Option<String>],
            &artists as &[Option<String>],
            &durations as &[Option<i64>],
            &fulls,
            &chans as &[Option<i64>],
            &msgs as &[Option<i64>],
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Guesses, likes and votes: replaced wholesale. A like can be taken back and
    /// a guess changed, so the sets are rewritten rather than merged; they are
    /// small, and this keeps the tables equal to the game rather than a superset.
    async fn save_reactions(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: i64,
    ) -> sqlx::Result<()> {
        sqlx::query!("DELETE FROM gp_guess WHERE game_id = $1", id)
            .execute(&mut **tx)
            .await?;
        sqlx::query!("DELETE FROM gp_like WHERE game_id = $1", id)
            .execute(&mut **tx)
            .await?;
        sqlx::query!("DELETE FROM gp_vote WHERE game_id = $1", id)
            .execute(&mut **tx)
            .await?;

        let mut g = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut l = (Vec::new(), Vec::new(), Vec::new());
        let mut v = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for t in &self.tracks {
            for (guesser, guessed) in &t.guesses {
                g.0.push(t.round_idx);
                g.1.push(t.submitter_id);
                g.2.push(*guesser);
                g.3.push(*guessed);
            }
            for user in &t.likes {
                l.0.push(t.round_idx);
                l.1.push(t.submitter_id);
                l.2.push(*user);
            }
            for (users, kind) in [(&t.skip_votes, VOTE_SKIP), (&t.full_votes, VOTE_FULL)] {
                for user in users {
                    v.0.push(t.round_idx);
                    v.1.push(t.submitter_id);
                    v.2.push(*user);
                    v.3.push(kind.to_string());
                }
            }
        }
        if !g.0.is_empty() {
            sqlx::query!(
                r#"INSERT INTO gp_guess (game_id, round_idx, submitter_id, guesser_id, guessed_id)
                   SELECT $1, * FROM UNNEST($2::int[], $3::bigint[], $4::bigint[], $5::bigint[])"#,
                id,
                &g.0,
                &g.1,
                &g.2,
                &g.3,
            )
            .execute(&mut **tx)
            .await?;
        }
        if !l.0.is_empty() {
            sqlx::query!(
                r#"INSERT INTO gp_like (game_id, round_idx, submitter_id, user_id)
                   SELECT $1, * FROM UNNEST($2::int[], $3::bigint[], $4::bigint[])"#,
                id,
                &l.0,
                &l.1,
                &l.2,
            )
            .execute(&mut **tx)
            .await?;
        }
        if !v.0.is_empty() {
            sqlx::query!(
                r#"INSERT INTO gp_vote (game_id, round_idx, submitter_id, user_id, kind)
                   SELECT $1, * FROM UNNEST($2::int[], $3::bigint[], $4::bigint[], $5::text[])"#,
                id,
                &v.0,
                &v.1,
                &v.2,
                &v.3,
            )
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    /// The guild's live game, if there is one.
    pub async fn load_live(pool: &PgPool, guild_id: i64) -> sqlx::Result<Option<GpLoaded>> {
        let Some(g) = sqlx::query!(
            r#"SELECT id, guild_id, started_at, host_id, voice_channel_id, text_channel_id,
                      category, phase, current_round, current_track, timer_secs,
                      clip_start_secs, clip_length_secs, generation,
                      EXTRACT(EPOCH FROM now() - last_seen_at)::bigint AS "last_seen_secs_ago!"
               FROM gp_game
               WHERE guild_id = $1 AND finished_at IS NULL
               ORDER BY started_at DESC
               LIMIT 1"#,
            guild_id,
        )
        .fetch_optional(pool)
        .await?
        else {
            return Ok(None);
        };
        let id = g.id;

        let players = sqlx::query!(
            r#"SELECT user_id, display_name, score FROM gp_player WHERE game_id = $1 ORDER BY user_id"#,
            id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| GpPlayerRow {
            user_id: r.user_id,
            display_name: r.display_name,
            score: r.score,
        })
        .collect();

        let rounds = sqlx::query!(
            r#"SELECT round_idx, prompt, closes_at, prompt_channel_id, prompt_message_id
               FROM gp_round WHERE game_id = $1 ORDER BY round_idx"#,
            id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| GpRoundRow {
            round_idx: r.round_idx,
            prompt: r.prompt,
            closes_at: r.closes_at,
            prompt_channel_id: r.prompt_channel_id,
            prompt_message_id: r.prompt_message_id,
        })
        .collect();

        let mut tracks: Vec<GpTrackRow> = sqlx::query!(
            r#"SELECT round_idx, submitter_id, position, url, title, artist, duration_secs,
                      play_full, message_channel_id, message_id
               FROM gp_track WHERE game_id = $1
               ORDER BY round_idx, position NULLS LAST, submitter_id"#,
            id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| GpTrackRow {
            round_idx: r.round_idx,
            submitter_id: r.submitter_id,
            position: r.position,
            url: r.url,
            title: r.title,
            artist: r.artist,
            duration_secs: r.duration_secs,
            play_full: r.play_full,
            message_channel_id: r.message_channel_id,
            message_id: r.message_id,
            guesses: Vec::new(),
            likes: Vec::new(),
            skip_votes: Vec::new(),
            full_votes: Vec::new(),
        })
        .collect();

        fn find(
            tracks: &mut [GpTrackRow],
            round_idx: i32,
            submitter_id: i64,
        ) -> Option<&mut GpTrackRow> {
            tracks
                .iter_mut()
                .find(|t| t.round_idx == round_idx && t.submitter_id == submitter_id)
        }
        for r in sqlx::query!(
            r#"SELECT round_idx, submitter_id, guesser_id, guessed_id FROM gp_guess
               WHERE game_id = $1 ORDER BY round_idx, submitter_id, guesser_id"#,
            id
        )
        .fetch_all(pool)
        .await?
        {
            if let Some(t) = find(&mut tracks, r.round_idx, r.submitter_id) {
                t.guesses.push((r.guesser_id, r.guessed_id));
            }
        }
        for r in sqlx::query!(
            r#"SELECT round_idx, submitter_id, user_id FROM gp_like
               WHERE game_id = $1 ORDER BY round_idx, submitter_id, user_id"#,
            id
        )
        .fetch_all(pool)
        .await?
        {
            if let Some(t) = find(&mut tracks, r.round_idx, r.submitter_id) {
                t.likes.push(r.user_id);
            }
        }
        for r in sqlx::query!(
            r#"SELECT round_idx, submitter_id, user_id, kind FROM gp_vote
               WHERE game_id = $1 ORDER BY round_idx, submitter_id, user_id"#,
            id
        )
        .fetch_all(pool)
        .await?
        {
            if let Some(t) = find(&mut tracks, r.round_idx, r.submitter_id) {
                match r.kind.as_str() {
                    VOTE_SKIP => t.skip_votes.push(r.user_id),
                    VOTE_FULL => t.full_votes.push(r.user_id),
                    _ => {},
                }
            }
        }

        Ok(Some(GpLoaded {
            saved: GpSaved {
                game: GpGameRow {
                    guild_id: g.guild_id,
                    started_at: g.started_at,
                    host_id: g.host_id,
                    voice_channel_id: g.voice_channel_id,
                    text_channel_id: g.text_channel_id,
                    category: g.category,
                    phase: g.phase,
                    current_round: g.current_round,
                    current_track: g.current_track,
                    timer_secs: g.timer_secs,
                    clip_start_secs: g.clip_start_secs,
                    clip_length_secs: g.clip_length_secs,
                    generation: g.generation,
                },
                players,
                rounds,
                tracks,
            },
            last_seen_secs_ago: g.last_seen_secs_ago,
        }))
    }
}

async fn mark_finished_in(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: i64,
    started_at: i64,
    outcome: GpOutcome,
) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        r#"UPDATE gp_game SET finished_at = now(), outcome = $3
           WHERE guild_id = $1 AND started_at = $2 AND finished_at IS NULL"#,
        guild_id,
        started_at,
        outcome.as_str(),
    )
    .execute(&mut **tx)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Take the game out of the live set. A game that never made it into the
/// database -- nobody submitted -- has no row to mark, and that is fine. Once
/// finished, a row stays finished: a later call with another outcome is a no-op,
/// so the first reason a game ended is the one that is kept.
pub async fn gp_mark_finished(
    pool: &PgPool,
    guild_id: i64,
    started_at: i64,
    outcome: GpOutcome,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let done = mark_finished_in(&mut tx, guild_id, started_at, outcome).await?;
    tx.commit().await?;
    Ok(done)
}

/// "Still here": bump `last_seen_at` on the live rows for these games.
pub async fn gp_heartbeat(pool: &PgPool, games: &[(i64, i64)]) -> sqlx::Result<u64> {
    if games.is_empty() {
        return Ok(0);
    }
    let guilds: Vec<i64> = games.iter().map(|(g, _)| *g).collect();
    let starts: Vec<i64> = games.iter().map(|(_, s)| *s).collect();
    let done = sqlx::query!(
        r#"UPDATE gp_game SET last_seen_at = now()
           WHERE finished_at IS NULL
             AND (guild_id, started_at) IN (SELECT * FROM UNNEST($1::bigint[], $2::bigint[]))"#,
        &guilds,
        &starts,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn track(round_idx: i32, submitter_id: i64, position: Option<i32>) -> GpTrackRow {
        GpTrackRow {
            round_idx,
            submitter_id,
            position,
            url: format!("https://www.youtube.com/watch?v=r{round_idx}u{submitter_id}"),
            title: Some(format!("song {submitter_id}")),
            artist: None,
            duration_secs: Some(200),
            play_full: false,
            message_channel_id: None,
            message_id: None,
            guesses: Vec::new(),
            likes: Vec::new(),
            skip_votes: Vec::new(),
            full_votes: Vec::new(),
        }
    }

    fn saved(guild_id: i64, started_at: i64) -> GpSaved {
        GpSaved {
            game: GpGameRow {
                guild_id,
                started_at,
                host_id: 100,
                voice_channel_id: 10,
                text_channel_id: 20,
                category: "nostalgia".into(),
                phase: "submitting".into(),
                current_round: 0,
                current_track: 0,
                timer_secs: 120,
                clip_start_secs: Some(30),
                clip_length_secs: Some(45),
                generation: 1,
            },
            players: vec![GpPlayerRow {
                user_id: 100,
                display_name: "alice".into(),
                score: 0,
            }],
            rounds: vec![
                GpRoundRow {
                    round_idx: 0,
                    prompt: "p0".into(),
                    closes_at: Some(started_at + 120),
                    prompt_channel_id: Some(20),
                    prompt_message_id: Some(1),
                },
                GpRoundRow {
                    round_idx: 1,
                    prompt: "p1".into(),
                    closes_at: None,
                    prompt_channel_id: None,
                    prompt_message_id: None,
                },
            ],
            tracks: vec![track(0, 100, None)],
        }
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_game_round_trips_and_a_second_save_replaces_the_first(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        let mut s = saved(1, 1_700_000_000);
        let id = s.save(&pool).await?;
        let loaded = GpSaved::load_live(&pool, 1).await?.expect("live");
        assert_eq!(loaded.saved, s);
        assert!(loaded.last_seen_secs_ago < 5);

        // The window closes: the submission becomes track 0, bob's song joins it,
        // and alice's earlier submission is the same row with a position now.
        s.game.phase = "playing".into();
        s.rounds[0].closes_at = None;
        s.tracks = vec![track(0, 200, Some(0)), track(0, 100, Some(1))];
        s.tracks[0].guesses = vec![(100, 200)];
        s.tracks[0].likes = vec![100];
        s.tracks[0].full_votes = vec![100];
        s.tracks[0].play_full = true;
        s.players.push(GpPlayerRow {
            user_id: 200,
            display_name: "bob".into(),
            score: 100,
        });
        assert_eq!(s.save(&pool).await?, id, "same game, same row");
        let loaded = GpSaved::load_live(&pool, 1).await?.expect("live").saved;
        assert_eq!(loaded, s);

        // A like taken back is gone, not lingering from the earlier write.
        s.tracks[0].likes.clear();
        s.save(&pool).await?;
        let loaded = GpSaved::load_live(&pool, 1).await?.expect("live").saved;
        assert!(loaded.tracks[0].likes.is_empty());
        assert_eq!(loaded.tracks[0].guesses, vec![(100, 200)]);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn finishing_leaves_history_and_no_live_game(pool: PgPool) -> sqlx::Result<()> {
        let s = saved(2, 1_700_000_000);
        s.save(&pool).await?;
        assert!(gp_mark_finished(&pool, 2, 1_700_000_000, GpOutcome::Ended).await?);
        assert!(GpSaved::load_live(&pool, 2).await?.is_none());
        // Already finished: the first reason stands and nothing is touched.
        assert!(!gp_mark_finished(&pool, 2, 1_700_000_000, GpOutcome::Lost).await?);
        let outcome: Option<String> = sqlx::query_scalar!(
            "SELECT outcome FROM gp_game WHERE guild_id = 2 AND started_at = 1700000000"
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(outcome.as_deref(), Some("ended"));
        // Nothing to mark for a game that was never written.
        assert!(!gp_mark_finished(&pool, 2, 1, GpOutcome::Abandoned).await?);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_new_game_in_the_guild_closes_a_stale_live_row_as_lost(
        pool: PgPool,
    ) -> sqlx::Result<()> {
        saved(3, 1_700_000_000).save(&pool).await?;
        saved(3, 1_700_009_000).save(&pool).await?;
        let live = GpSaved::load_live(&pool, 3).await?.expect("live").saved;
        assert_eq!(live.game.started_at, 1_700_009_000);
        let outcome: Option<String> = sqlx::query_scalar!(
            "SELECT outcome FROM gp_game WHERE guild_id = 3 AND started_at = 1700000000"
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(outcome.as_deref(), Some("lost"));
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_finished_phase_is_written_as_finished(pool: PgPool) -> sqlx::Result<()> {
        let mut s = saved(4, 1_700_000_000);
        s.game.phase = "finished".into();
        s.save(&pool).await?;
        assert!(GpSaved::load_live(&pool, 4).await?.is_none());
        let outcome: Option<String> =
            sqlx::query_scalar!("SELECT outcome FROM gp_game WHERE guild_id = 4")
                .fetch_one(&pool)
                .await?;
        assert_eq!(outcome.as_deref(), Some("finished"));
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn heartbeat_touches_only_the_named_live_games(pool: PgPool) -> sqlx::Result<()> {
        saved(5, 1_700_000_000).save(&pool).await?;
        saved(6, 1_700_000_000).save(&pool).await?;
        sqlx::query!("UPDATE gp_game SET last_seen_at = now() - interval '10 minutes'")
            .execute(&pool)
            .await?;
        assert_eq!(gp_heartbeat(&pool, &[(5, 1_700_000_000), (7, 1)]).await?, 1);
        let five = GpSaved::load_live(&pool, 5).await?.expect("live");
        let six = GpSaved::load_live(&pool, 6).await?.expect("live");
        assert!(five.last_seen_secs_ago < 5);
        assert!(six.last_seen_secs_ago > 500);
        assert_eq!(gp_heartbeat(&pool, &[]).await?, 0);
        Ok(())
    }
}
