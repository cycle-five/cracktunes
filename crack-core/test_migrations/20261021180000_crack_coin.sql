-- CrackCoin: a member's linked devnet address and their withdrawals.
-- Who withdraws where. One address per Discord user, used in every server.
CREATE TABLE IF NOT EXISTS coin_links (
    user_id   BIGINT PRIMARY KEY,
    address   TEXT NOT NULL,          -- base58, validated on the way in
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS coin_withdrawals (
    id                      BIGSERIAL PRIMARY KEY,
    guild_id                BIGINT NOT NULL,
    user_id                 BIGINT NOT NULL,
    address                 TEXT   NOT NULL,   -- snapshot: a relink doesn't move it
    mint                    TEXT   NOT NULL,   -- snapshot of CRACK_COIN_MINT
    amount                  BIGINT NOT NULL CHECK (amount > 0),
    state                   TEXT   NOT NULL DEFAULT 'pending',  -- pending | done | refunded
    signature               TEXT,   -- base58; stored BEFORE the transaction is sent
    tx                      BYTEA,  -- the signed transaction, for rebroadcast
    last_valid_block_height BIGINT,
    genesis_hash            TEXT,
    sponsored               BOOLEAN NOT NULL DEFAULT false,  -- this tx creates the token account
    reason                  TEXT,   -- why it was refunded, for the member and for us
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at                TIMESTAMPTZ
);

-- One withdrawal in flight per member, across all servers.
CREATE UNIQUE INDEX IF NOT EXISTS coin_withdrawals_one_pending
    ON coin_withdrawals (user_id) WHERE state = 'pending';

-- For the resolver's sweep.
CREATE INDEX IF NOT EXISTS coin_withdrawals_state ON coin_withdrawals (state);

ALTER TABLE economy_ledger
    ADD COLUMN IF NOT EXISTS coin_withdrawal_id BIGINT REFERENCES coin_withdrawals (id);
