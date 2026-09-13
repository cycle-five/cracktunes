-- crack-musicreco's cache and daily call budget (spec §7). The Rust side is
-- crack-core/src/db/musicreco.rs.
--
-- Keys are NORMALIZED (trim, collapse whitespace, lowercase, fold a curly
-- apostrophe to a straight one) so `Queen ` and `queen` are one entry rather
-- than two metered calls. The values sent to the provider are the originals.
CREATE TABLE IF NOT EXISTS musicreco_cache (
    provider   TEXT        NOT NULL,
    artist     TEXT        NOT NULL,
    track      TEXT        NOT NULL,
    -- A serialized Vec<Recommendation>, `[]` when none. Written and read only
    -- whole; the database never queries into it.
    results    JSONB       NOT NULL,
    -- 🔑 false rows are the point: a seed that returned nothing cost a call,
    -- and without a negative entry it costs one every time that track ends.
    found      BOOLEAN     NOT NULL,
    -- Read as well as written: an entry older than its TTL (30 days found,
    -- 7 days not found) is a miss, and the next write replaces it.
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, artist, track)
);

-- No rate-limit header exists to read, so usage is counted locally. `day` is
-- the UTC date. Rows for past days are retained: they are the only record of
-- actual quota usage.
CREATE TABLE IF NOT EXISTS musicreco_budget (
    provider TEXT    NOT NULL,
    day      DATE    NOT NULL,
    calls    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider, day)
);
