-- Guilds table
CREATE TABLE IF NOT EXISTS guild (
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    guild_id    TEXT NOT NULL,
    name        TEXT NOT NULL
);