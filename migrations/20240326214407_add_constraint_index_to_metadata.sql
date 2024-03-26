-- Add an index to the metadata table to speed up queries
CREATE UNIQUE INDEX metadata_track_artist_album_idx
ON  metadata (track, artist, album);