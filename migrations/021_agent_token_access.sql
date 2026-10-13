-- Agent token access: write (the default) or read. Existing tokens read as write.
-- Adding a column fires no row or statement trigger, so the append-only triggers stay in effect.
ALTER TABLE joppa.agent_tokens
    ADD COLUMN IF NOT EXISTS access text NOT NULL DEFAULT 'write' CHECK (access IN ('write', 'read'));
