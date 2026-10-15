-- Workspace access by email domain: the owner names a domain, and a verified Google sign-in
-- with an email in that domain enrolls as a human member. Keys are lowercase domains.
CREATE TABLE IF NOT EXISTS joppa.domain_grants (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    domain text NOT NULL CHECK (domain = lower(domain) AND position('.' IN domain) > 0 AND position('@' IN domain) = 0),
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (workspace, domain)
);
CREATE INDEX IF NOT EXISTS domain_grants_domain ON joppa.domain_grants(domain);
REVOKE ALL ON joppa.domain_grants FROM PUBLIC;
