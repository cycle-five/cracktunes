-- Playlists table
CREATE TABLE IF NOT EXISTS playlist (
    id SERIAL PRIMARY KEY,
    "name" TEXT NOT NULL,
    user_id BIGINT,
    privacy TEXT DEFAULT 'private' NOT NULL CHECK (privacy IN ('public', 'private', 'shared')),
    CONSTRAINT fk_playlist_user FOREIGN KEY (user_id) REFERENCES "user"(id)
);