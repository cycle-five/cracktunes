-- Playlists table
CREATE TABLE IF NOT EXISTS playlist (
    id      INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name    TEXT NOT NULL,
    user_id INTEGER,
    FOREIGN KEY (user_id) REFERENCES user(id)
);