-- Add migration script here
CREATE TABLE IF NOT EXISTS shared_playlist (
    id SERIAL PRIMARY KEY,
    playlist_id BIGINT REFERENCES playlist(id),
    shared_with_user_id BIGINT REFERENCES public.user(id),
    CONSTRAINT uq_shared_playlist UNIQUE (playlist_id, shared_with_user_id)
);