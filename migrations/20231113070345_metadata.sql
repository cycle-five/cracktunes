-- Add migration script here
CREATE TABLE IF NOT EXISTS metadata (
    id SERIAL PRIMARY KEY,
    track TEXT,
    artist TEXT,
    album TEXT,
    date DATE,
    channels SMALLINT,
    channel TEXT,
    start_time INTERVAL,
    duration INTERVAL,
    sample_rate INTEGER,
    source_url TEXT,
    title TEXT,
    thumbnail TEXT
);