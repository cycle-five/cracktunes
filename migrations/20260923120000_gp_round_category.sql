-- `/gp start` with no category plays 🎲 Random: a different category each
-- round, shown with that round's prompt, so it has to survive a restart too.
ALTER TABLE gp_round
    -- GpCategory::slug(). NULL on a round saved before this column existed,
    -- whose category is the game's.
    ADD COLUMN IF NOT EXISTS category TEXT;
