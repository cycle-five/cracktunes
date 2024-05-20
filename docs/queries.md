# Queries
# =======
This documents contains a list of all the queries that are used in the code
for testing purposes. They are broken down by sections by the file in which
they appear.

## Playlist (src/db/playlist.rs)

### Playlist::create
`name: &str, user_id: i64`
```sql
INSERT INTO playlist (name, user_id) VALUES ($1, $2) RETURNING id, name, user_id, privacy;
```

### Playlist::get_playlist_by_id
`id: i32`
```sql
SELECT * FROM playlist WHERE id = $1
```

### Playlist::get_playlists_by_user_id
`user_id: i64`
```sql
SELECT * FROM playlist WHERE user_id = $1;
```

### Playlist::get_playlist_by_name
`name: &str, user_id: i64`
```sql
SELECT * FROM playlist WHERE user_id = $1 and name = $2;
```

### get_track_metadata_for_playlist_name
```sql
SELECT
    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
FROM
    (metadata INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id INNER JOIN playlist ON playlist_track.playlist_id = playlist.id)
WHERE playlist.name = $1 AND playlist.user_id = $2
```