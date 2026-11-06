-- The alias of a write: the name of the agent that writes under the account of the actor
-- (src/actor_alias.rs). Older events and marks have none.
ALTER TABLE joppa.events ADD COLUMN IF NOT EXISTS alias text
    CHECK (alias IS NULL OR char_length(alias) BETWEEN 1 AND 40);
ALTER TABLE joppa.history_marks ADD COLUMN IF NOT EXISTS alias text
    CHECK (alias IS NULL OR char_length(alias) BETWEEN 1 AND 40);
