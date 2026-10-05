CREATE TABLE IF NOT EXISTS joppa.object_aliases (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    address text NOT NULL,
    target text NOT NULL,
    PRIMARY KEY (workspace,address),
    UNIQUE (workspace,target)
);
REVOKE ALL ON joppa.object_aliases FROM PUBLIC;
