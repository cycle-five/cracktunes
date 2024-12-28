-- Playlist Tracks junction table
CREATE TABLE IF NOT EXISTS playlist_track (
    id SERIAL PRIMARY KEY NOT NULL,
    playlist_id INTEGER NOT NULL,
    metadata_id INTEGER NOT NULL,
    guild_id BIGINT,
    channel_id BIGINT,
    CONSTRAINT fk_playlist_track_playlist FOREIGN KEY (playlist_id) REFERENCES playlist(id),
    FOREIGN KEY (metadata_id) REFERENCES metadata(id)
);