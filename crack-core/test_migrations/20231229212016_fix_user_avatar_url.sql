-- This field isn't allowed to be null, so set a default value
ALTER TABLE public."user"
ALTER COLUMN avatar_url
SET DEFAULT '';