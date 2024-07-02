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

### Playlist::add_track
`playlist_id: i32, metadata_id: i32, guild_id: i64, channel_id: i64`
```sql
INSERT INTO playlist_track (playlist_id, metadata_id, guild_id, channel_id) VALUES ($1, $2, $3, $4);
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

### Playlist::update_playlist_name
`id: i32, new_name: &str`
```sql
UPDATE playlist SET name = $1 WHERE id = $2 RETURNING id, name, user_id, privacy
```

### Playlist::delete_playlist
`playlist_id: i32`
```sql
DELETE FROM playlist
WHERE id = $1
```

### Playlist::get_track_metadata_for_playlist_name
`playlist_name: &str, user_id: i64`
```sql
SELECT
    metadata.id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
FROM
    (metadata INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id INNER JOIN playlist ON playlist_track.playlist_id = playlist.id)
WHERE playlist.name = $1 AND playlist.user_id = $2
```

### Playlist::delete_playlist_by_name
`playlist_name: &str, user_id: i64`
```sql
SELECT id FROM playlist
WHERE name = $1 AND user_id = $2
```

## Playlog (src/db/playlog.rs)

### Playlog::create
`user_id: i64, guild_id: i64, metadata_id: i32`
```sql
INSERT INTO play_log (user_id, guild_id, metadata_id)
VALUES ($1, $2, $3)
RETURNING id, user_id, guild_id, metadata_id, created_at
```

### Playlog::get_last_played_by_user_id
`guild_id: i64, max_dislikes: i32`
```sql
select title, artist 
from (play_log
    join metadata on 
    play_log.metadata_id = metadata.id)
    left join track_reaction on play_log.id = track_reaction.play_log_id
where guild_id = $1 and (track_reaction is null or track_reaction.dislikes >= $2)
order by play_log.created_at desc limit 5
```

### Playlog::get_last_played_by_guild_metadata
`guild_id: i64`
```sql
select metadata.id, title, artist, album, track, date, channels, channel, start_time, duration, sample_rate, source_url, thumbnail
from play_log 
join metadata on 
play_log.metadata_id = metadata.id 
where guild_id = $1 order by created_at desc limit 5
```

### Playlog::get_last_played_by_user
`user_id: i64`
```sql
select title, artist 
from play_log 
join metadata on 
play_log.metadata_id = metadata.id 
where user_id = $1 order by created_at desc limit 5
```

## Metadata (src/db/metadata.rs)

### Metadata::get_or_create
`source_url: &str`
```sql
SELECT
    metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
FROM 
    metadata
WHERE 
    metadata.source_url = $1
```
`track: &str, artist: &str, album: &str, date: &str, channels: i32, channel: &str, start_time: i32, duration: i32, sample_rate: i32, source_url: &str, title: &str, thumbnail: &str`
```sql
INSERT INTO
    metadata (track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
    RETURNING id, track, artist, album, date, channels, channel, start_time, duration, sample_rate, source_url, title, thumbnail
```

### Metadata::get_by_url
`source_url: &str`
```sql
SELECT
    metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
FROM 
    metadata
WHERE 
    metadata.source_url = $1
```

### Metadata::playlist_track_to_metadata
`playlist_track_id: i32`
```sql
SELECT
    metadata.id, metadata.track, metadata.artist, metadata.album, metadata.date, metadata.channels, metadata.channel, metadata.start_time, metadata.duration, metadata.sample_rate, metadata.source_url, metadata.title, metadata.thumbnail
    FROM metadata
    INNER JOIN playlist_track ON playlist_track.metadata_id = metadata.id
    WHERE playlist_track.id = $1
```