-- `/buttons` turns the now-playing message's buttons off or on for a server.
-- On by default: the buttons were always shown before v0.25.0.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS now_playing_buttons BOOLEAN NOT NULL DEFAULT TRUE;
