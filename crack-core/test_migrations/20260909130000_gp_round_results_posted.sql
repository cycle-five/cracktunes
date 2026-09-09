-- A round's results embed is posted after the snapshot that moves the game
-- past the round. If the bot goes down in between, the results never reach
-- the channel -- and with the reveal held to the round's end (reveal:round)
-- that embed is the only place the round's submitters are ever named. Record
-- when it has been posted, so a resume can post it if it has not been.
ALTER TABLE gp_round
    -- The column default is TRUE: a round saved before this column existed was
    -- played by a bot that could not replay results, and a resume must not
    -- post them a second time on the strength of a missing flag.
    ADD COLUMN IF NOT EXISTS results_posted BOOLEAN NOT NULL DEFAULT TRUE;
