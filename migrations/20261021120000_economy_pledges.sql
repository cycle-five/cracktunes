-- Economy, part 3: crowdfunds. A pledge is one member's share of a fund, and a
-- fund is an economy_actions row in state 'funding'. Additive: an older image
-- ignores both the table and the index.
CREATE TABLE IF NOT EXISTS economy_pledges (
    id          BIGSERIAL PRIMARY KEY,
    action_id   BIGINT NOT NULL,          -- the economy_actions row in state 'funding'
    guild_id    BIGINT NOT NULL,
    user_id     BIGINT NOT NULL,
    amount      BIGINT NOT NULL CHECK (amount > 0),
    signed      BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    refunded_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS economy_pledges_action ON economy_pledges (action_id);
CREATE UNIQUE INDEX IF NOT EXISTS economy_actions_one_fund
    ON economy_actions (guild_id, target_id, kind) WHERE state = 'funding';
