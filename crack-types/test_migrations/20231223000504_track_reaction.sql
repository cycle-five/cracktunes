-- Table to hold user reactions to tracks played, including likes and dislikes, and skip votes.
CREATE TABLE IF NOT EXISTS track_reaction (
    play_log_id INTEGER PRIMARY KEY NOT NULL,
    likes INTEGER NOT NULL DEFAULT 0,
    dislikes INTEGER NOT NULL DEFAULT 0,
    skip_votes INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_track_reaction_play_log FOREIGN KEY (play_log_id) REFERENCES play_log (id)
);