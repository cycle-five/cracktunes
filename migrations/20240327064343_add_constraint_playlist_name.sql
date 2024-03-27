-- Add migration script here
CREATE UNIQUE INDEX playlist_name_user_id_idx ON playlist ("name", user_id);