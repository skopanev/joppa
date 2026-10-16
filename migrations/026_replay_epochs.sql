-- Replay epochs: the first journal position at which a replay rule applies in a workspace.
-- A replay rule is a later change of the meaning of an old command (src/commands.rs
-- REPLAY_RULES). A replay gives each older event the meaning that it had when it was
-- written. A stored epoch never changes. A journal replay refuses a workspace that has
-- events and lacks an epoch for a rule.
CREATE TABLE IF NOT EXISTS joppa.replay_epochs (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    rule text NOT NULL,
    first_position bigint NOT NULL CHECK (first_position >= 1),
    PRIMARY KEY (workspace, rule)
);
-- Seed the journals that existed before this table from deployment evidence. Each rule
-- applies from the first event recorded after the first service start of the build that
-- added it, or after the last event when no later event exists:
-- reset_changed (885a40d) and keep_assessments (9a240fc, 68da2f6): deploy 59ab018,
-- joppa.service start 2026-10-15 06:53:52Z.
-- list_save_reset (b08093a): deploy 7c372be, joppa.service start 2026-10-15 19:36:49Z.
-- A workspace that this server creates stores its epochs at creation, so it never gets a seed.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, r.rule, COALESCE(
        (SELECT min(e.position) FROM joppa.events e WHERE e.workspace = w.id AND e.recorded_at >= r.live_from),
        (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id))
FROM joppa.workspaces w
CROSS JOIN (VALUES
    ('reset_changed', timestamptz '2026-10-15 06:53:52+00'),
    ('keep_assessments', timestamptz '2026-10-15 06:53:52+00'),
    ('list_save_reset', timestamptz '2026-10-15 19:36:49+00')) AS r(rule, live_from)
ON CONFLICT (workspace, rule) DO NOTHING;
