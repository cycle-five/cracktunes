-- channel table
CREATE TABLE IF NOT EXISTS channels(
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    channel_id  TEXT NOT NULL,
    name        TEXT,
    guild_id    TEXT NOT NULL,
    FOREIGN KEY (guild_id) REFERENCES guild(id)
);