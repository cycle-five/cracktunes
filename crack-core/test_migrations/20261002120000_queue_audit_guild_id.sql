-- Keyset paging for the dashboard's history page: newest first by insertion
-- order (id), per guild. Spec: docs/superpowers/specs/2026-10-02-dashboard-history-design.md
CREATE INDEX IF NOT EXISTS queue_audit_guild_id ON queue_audit (guild_id, id DESC);
