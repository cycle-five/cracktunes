# Queries
# =======
This documents contains a list of all the queries that are used in the code
for testing purposes. They are broken down by sections by the file in which
they appear.

## Playlist (src/db/playlist.rs)

### get_track_metadata_for_playlist_name
```sql
SELECT
    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
FROM
    (metadata INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id INNER JOIN playlist ON playlist_track.playlist_id = playlist.id)
WHERE playlist.name = $1 AND playlist.user_id = $2
```