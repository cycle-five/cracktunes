CREATE TABLE IF NOT EXISTS economy_settings (
    guild_id   BIGINT PRIMARY KEY,
    enabled    BOOLEAN NOT NULL DEFAULT false,
    -- EconomyConfig (crack-core economy::settings), typed serde; missing fields take defaults.
    config     JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS economy_wallets (
    guild_id          BIGINT NOT NULL,
    user_id           BIGINT NOT NULL,
    balance           BIGINT NOT NULL DEFAULT 0 CHECK (balance >= 0),
    earned_voice      BIGINT NOT NULL DEFAULT 0,
    earned_messages   BIGINT NOT NULL DEFAULT 0,
    opted_out_at      TIMESTAMPTZ,
    optout_changed_at TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX IF NOT EXISTS economy_wallets_top ON economy_wallets (guild_id, balance DESC);
CREATE TABLE IF NOT EXISTS economy_ledger (
    id         BIGSERIAL PRIMARY KEY,
    guild_id   BIGINT NOT NULL,
    user_id    BIGINT NOT NULL,
    delta      BIGINT NOT NULL,
    kind       TEXT NOT NULL,      -- LedgerKind
    action_id  BIGINT,
    by_user_id BIGINT,             -- adjust: who
    reason     TEXT,               -- adjust: why
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS economy_ledger_user ON economy_ledger (guild_id, user_id, created_at DESC);
CREATE TABLE IF NOT EXISTS economy_actions (
    id          BIGSERIAL PRIMARY KEY,
    guild_id    BIGINT NOT NULL,
    kind        TEXT NOT NULL,     -- ActionKind::key
    buyer_id    BIGINT NOT NULL,
    target_id   BIGINT,
    price       BIGINT NOT NULL,
    note        TEXT,
    signed      BOOLEAN NOT NULL DEFAULT false,
    payload     JSONB NOT NULL,    -- ActionPayload, typed serde
    state       TEXT NOT NULL,     -- ActionState
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ,
    ended_at    TIMESTAMPTZ,
    end_reason  TEXT,
    revealed_at TIMESTAMPTZ,
    revealed_by BIGINT
);
CREATE INDEX IF NOT EXISTS economy_actions_live ON economy_actions (guild_id, state)
    WHERE state IN ('pending', 'active', 'ending');
CREATE INDEX IF NOT EXISTS economy_actions_target ON economy_actions (guild_id, target_id, created_at DESC);
CREATE TABLE IF NOT EXISTS economy_immunity (
    guild_id   BIGINT NOT NULL,
    user_id    BIGINT NOT NULL,
    source     TEXT NOT NULL CHECK (source IN ('bought', 'mod')),
    until      TIMESTAMPTZ,        -- NULL: until lifted (mod grants only)
    granted_by BIGINT,
    reason     TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id, source)
);
