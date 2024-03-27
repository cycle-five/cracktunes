-- Add an index to the metadata table to speed up queries
CREATE INDEX metadata_track_artist_album_idx
ON  metadata (source_url);