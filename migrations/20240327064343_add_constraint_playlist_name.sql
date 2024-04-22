-- Add migration script here
CREATE INDEX playlist_name_user_id_idx ON playlist ("name", user_id);