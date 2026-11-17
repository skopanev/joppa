-- An email grant names the role of its person: viewer (Read-only) or human (Editor). The grants
-- from before this migration keep human. A new grant is Read-only unless an Editor picks Editor.
ALTER TABLE joppa.email_grants ADD COLUMN IF NOT EXISTS role text NOT NULL DEFAULT 'human'
    CHECK (role IN ('viewer', 'human'));
ALTER TABLE joppa.email_grants ALTER COLUMN role SET DEFAULT 'viewer';
