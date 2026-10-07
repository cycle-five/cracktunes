-- Whether a dashboard control or a now-playing button press posts an echo
-- line ("⏭ Skipped **Title** — @user") in the channel. On by default, the
-- behaviour before v0.23.0. See
-- docs/superpowers/specs/2026-10-07-messaging-layer-and-now-playing-buttons-design.md.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS control_echoes BOOLEAN NOT NULL DEFAULT TRUE;
