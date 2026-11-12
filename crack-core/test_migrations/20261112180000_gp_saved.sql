-- Stable ownership for automatic lists without reserving a user-facing name.
CREATE TABLE gp_saved_playlist (
    user_id BIGINT PRIMARY KEY REFERENCES "user"(id),
    playlist_id INTEGER NOT NULL UNIQUE REFERENCES playlist(id) ON DELETE CASCADE
);
