-- Add migration script here
CREATE TABLE IF NOT EXISTS shared_playlist (
    id SERIAL PRIMARY KEY,
    playlist_id INT REFERENCES playlist(id),
    shared_with_user_id INT REFERENCES user(id),
    UNIQUE (playlist_id, shared_with_user_id)
);