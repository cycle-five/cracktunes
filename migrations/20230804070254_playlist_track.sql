-- Playlist Tracks junction table
CREATE TABLE IF NOT EXISTS playlist_track (
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    playlist_id INTEGER NOT NULL,
    metadata_id INTEGER NOT NULL,
    guild_id    INTEGER,
    channel_id  INTEGER,
    FOREIGN KEY (playlist_id) REFERENCES playlist(id),
    FOREIGN KEY (metadata_id) REFERENCES metadata(id)
);