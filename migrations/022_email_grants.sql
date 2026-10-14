-- Workspace access by email: the owner names a Google email, and a verified sign-in
-- with that email enrolls as a human member. Keys are lowercase emails.
CREATE TABLE IF NOT EXISTS joppa.email_grants (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    email text NOT NULL CHECK (position('@' IN email) > 1),
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (workspace, email)
);
CREATE INDEX IF NOT EXISTS email_grants_email ON joppa.email_grants(email);
REVOKE ALL ON joppa.email_grants FROM PUBLIC;
