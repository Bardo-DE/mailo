-- Which act queued each entry: the id of the patch the act applied. Taking the act back finds
-- the entry it left by this id, in the same transaction that writes the act's inverse, and not
-- by comparing undo patches, which a pass that is running at that moment can race.
--
-- Null for entries queued before this, and for work that no act queued; taking an act back
-- finds those as it did, by their undo.
ALTER TABLE outbox ADD COLUMN forward_id TEXT;
CREATE INDEX outbox_forward ON outbox(forward_id) WHERE forward_id IS NOT NULL;
