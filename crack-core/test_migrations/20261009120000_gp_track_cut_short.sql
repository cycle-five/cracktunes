-- A song whose stream died part-way, before the played bar, still pays its
-- guesses and likes but not the fooled-everyone bonus (#423). Round results
-- derive each song's payout again from the song, so this has to survive a
-- restart with the game. Never true together with `failed`.
ALTER TABLE gp_track
    ADD COLUMN IF NOT EXISTS cut_short BOOLEAN NOT NULL DEFAULT FALSE;
