-- The paired deck, private assignments and ballots for an in-progress Wrong Prompt game.
-- Classic games leave the column NULL.
ALTER TABLE gp_game ADD COLUMN wrong_prompt JSONB;
