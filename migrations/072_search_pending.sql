-- The objects whose search rows a write leaves for later. A write adds one row for each
-- object that it reaches, and the address * for a write that reaches every object. The
-- background pass and each search read build the rows of these objects and remove them.
CREATE TABLE IF NOT EXISTS joppa.search_pending (
    workspace text NOT NULL REFERENCES joppa.workspaces(id) ON DELETE CASCADE,
    address text NOT NULL,
    PRIMARY KEY (workspace, address)
);
