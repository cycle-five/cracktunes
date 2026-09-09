-- `/gp start ... reveal:round` holds every submitter's name until the round's
-- last song has played (#450), and the round's results say which songs never
-- played (#433). Both have to survive a restart with the game.
ALTER TABLE gp_game
    -- song | round: GpReveal::slug()
    ADD COLUMN IF NOT EXISTS reveal TEXT NOT NULL DEFAULT 'song';

ALTER TABLE gp_track
    -- songbird could not open the stream: nobody heard it, nothing was scored.
    ADD COLUMN IF NOT EXISTS failed BOOLEAN NOT NULL DEFAULT FALSE;
