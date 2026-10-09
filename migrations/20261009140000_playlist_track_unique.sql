-- Saving a song twice must not insert it twice. Keep the earliest row when a
-- list already has the same track more than once, then refuse another copy.
DELETE FROM playlist_track
WHERE id IN (
    SELECT id FROM (
        SELECT id,
               ROW_NUMBER() OVER (PARTITION BY playlist_id, metadata_id ORDER BY id) AS rn
        FROM playlist_track
    ) ranked
    WHERE rn > 1
);

ALTER TABLE playlist_track
    ADD CONSTRAINT playlist_track_playlist_id_metadata_id_key
    UNIQUE (playlist_id, metadata_id);
