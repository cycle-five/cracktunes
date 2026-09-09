-- `/gp` games, saved as they run so a redeploy or a crash does not end them.
--
-- One row per game. The live game for a guild is the row whose finished_at is
-- NULL; every other row is history. A game is written at two points -- when a
-- song is submitted and when a song ends -- and left alone in between, so the
-- rows describe the game as it stood at the last of those. Guesses, likes and
-- votes on the song that is still playing are not here: after a resume that
-- song plays again from the top and the room casts them again.
CREATE TABLE IF NOT EXISTS gp_game (
    id BIGSERIAL PRIMARY KEY,
    guild_id BIGINT NOT NULL,
    -- Unix seconds of `/gp start`. With guild_id, names the game across restarts.
    started_at BIGINT NOT NULL,
    host_id BIGINT NOT NULL,
    voice_channel_id BIGINT NOT NULL,
    text_channel_id BIGINT NOT NULL,
    -- GpCategory::slug()
    category TEXT NOT NULL,
    -- submitting | playing | finished
    phase TEXT NOT NULL,
    current_round INTEGER NOT NULL,
    current_track INTEGER NOT NULL,
    timer_secs BIGINT NOT NULL,
    clip_start_secs BIGINT,
    clip_length_secs BIGINT,
    generation BIGINT NOT NULL,
    -- Bumped on every write, and on every live game when the bot shuts down
    -- cleanly. A song whose game was not seen for GP_RESUME_WINDOW_SECS is not
    -- resumed; an open submission window is judged by closes_at instead.
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    -- finished | ended | abandoned | lost
    outcome TEXT,
    UNIQUE (guild_id, started_at)
);

CREATE INDEX IF NOT EXISTS gp_game_live ON gp_game (guild_id) WHERE finished_at IS NULL;

-- Everyone who has submitted, guessed or liked, with the display name seen and
-- the score as it stood.
CREATE TABLE IF NOT EXISTS gp_player (
    game_id BIGINT NOT NULL REFERENCES gp_game (id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL,
    display_name TEXT NOT NULL,
    score INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (game_id, user_id)
);

CREATE TABLE IF NOT EXISTS gp_round (
    game_id BIGINT NOT NULL REFERENCES gp_game (id) ON DELETE CASCADE,
    round_idx INTEGER NOT NULL,
    prompt TEXT NOT NULL,
    -- Unix seconds; set while the submission window is open.
    closes_at BIGINT,
    prompt_channel_id BIGINT,
    prompt_message_id BIGINT,
    PRIMARY KEY (game_id, round_idx)
);

-- One song per player per round: a submission while the window is open, and
-- a track in play order (position) once it has closed. Resubmitting replaces.
CREATE TABLE IF NOT EXISTS gp_track (
    game_id BIGINT NOT NULL,
    round_idx INTEGER NOT NULL,
    submitter_id BIGINT NOT NULL,
    position INTEGER,
    url TEXT NOT NULL,
    title TEXT,
    artist TEXT,
    duration_secs BIGINT,
    play_full BOOLEAN NOT NULL DEFAULT FALSE,
    message_channel_id BIGINT,
    message_id BIGINT,
    PRIMARY KEY (game_id, round_idx, submitter_id),
    FOREIGN KEY (game_id, round_idx) REFERENCES gp_round (game_id, round_idx) ON DELETE CASCADE
);

-- guesser -> guessed submitter; the last guess wins, so one row per guesser.
CREATE TABLE IF NOT EXISTS gp_guess (
    game_id BIGINT NOT NULL,
    round_idx INTEGER NOT NULL,
    submitter_id BIGINT NOT NULL,
    guesser_id BIGINT NOT NULL,
    guessed_id BIGINT NOT NULL,
    PRIMARY KEY (game_id, round_idx, submitter_id, guesser_id),
    FOREIGN KEY (game_id, round_idx, submitter_id)
        REFERENCES gp_track (game_id, round_idx, submitter_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS gp_like (
    game_id BIGINT NOT NULL,
    round_idx INTEGER NOT NULL,
    submitter_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    PRIMARY KEY (game_id, round_idx, submitter_id, user_id),
    FOREIGN KEY (game_id, round_idx, submitter_id)
        REFERENCES gp_track (game_id, round_idx, submitter_id) ON DELETE CASCADE
);

-- skip: `/gp voteskip`; full: `/gp votefull`.
CREATE TABLE IF NOT EXISTS gp_vote (
    game_id BIGINT NOT NULL,
    round_idx INTEGER NOT NULL,
    submitter_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('skip', 'full')),
    PRIMARY KEY (game_id, round_idx, submitter_id, user_id, kind),
    FOREIGN KEY (game_id, round_idx, submitter_id)
        REFERENCES gp_track (game_id, round_idx, submitter_id) ON DELETE CASCADE
);
