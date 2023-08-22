-- Playlists table
CREATE TABLE IF NOT EXISTS playlist (
    id      INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name    TEXT NOT NULL,
    user_id INTEGER,
    privacy TEXT DEFAULT 'private' NOT NULL CHECK (privacy IN ('public', 'private', 'shared')),
    FOREIGN KEY (user_id) REFERENCES user(id)
);