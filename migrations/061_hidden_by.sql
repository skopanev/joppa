-- The hidden flag of a workspace is per member (Workspace.hidden_by). A meta row or a board row
-- of the older release holds the key hidden. This pass gives such a workspace the hidden_by
-- that a replay gives, without a replay: update_workspace with hidden adds or removes its actor,
-- and remove_member removes its member. The last of these events for a member decides. The
-- meta row and the board state keep hidden_by only when it is not empty, as serde writes them.
-- A workspace without the key gets no write, so a second run writes nothing.
WITH old AS (
    SELECT b.workspace FROM joppa.board_cache b
    WHERE b.first_view ? 'hidden' OR b.state ? 'hidden' OR EXISTS (
        SELECT 1 FROM joppa.state_entries s
        WHERE s.workspace = b.workspace AND s.section = 'meta' AND s.key = '' AND s.value ? 'hidden')
), flags AS (
    SELECT DISTINCT ON (e.workspace, f.member) e.workspace, f.member, f.hidden
    FROM joppa.events e JOIN old o ON o.workspace = e.workspace
    CROSS JOIN LATERAL (VALUES (
        CASE e.command->>'op' WHEN 'remove_member' THEN e.command->>'id' ELSE e.actor END,
        e.command->'hidden' = 'true'::jsonb)) f(member, hidden)
    WHERE e.command->>'op' = 'remove_member'
        OR (e.command->>'op' = 'update_workspace' AND jsonb_typeof(e.command->'hidden') = 'boolean')
    ORDER BY e.workspace, f.member, e.position DESC
), moved AS (
    -- A BTreeSet gives its members in byte order.
    SELECT o.workspace, COALESCE(jsonb_agg(f.member ORDER BY f.member COLLATE "C")
        FILTER (WHERE f.hidden), '[]'::jsonb) AS hidden_by
    FROM old o LEFT JOIN flags f ON f.workspace = o.workspace
    GROUP BY o.workspace
), meta AS (
    SELECT m.workspace, m.hidden_by, CASE WHEN m.hidden_by = '[]'::jsonb THEN '{}'::jsonb
        ELSE jsonb_build_object('hidden_by', m.hidden_by) END AS field
    FROM moved m
), meta_rows AS (
    UPDATE joppa.state_entries s
    SET value = (s.value - '{hidden,hidden_by}'::text[]) || m.field
    FROM meta m
    WHERE s.workspace = m.workspace AND s.section = 'meta' AND s.key = ''
        AND s.value IS DISTINCT FROM (s.value - '{hidden,hidden_by}'::text[]) || m.field
), boards AS (
    -- An empty board row stays empty: the backfill rebuilds it from the journal.
    SELECT b.workspace,
        CASE WHEN b.first_view ? 'object_types'
            THEN (b.first_view - 'hidden') || jsonb_build_object('hidden_by', m.hidden_by)
            ELSE b.first_view END AS first_view,
        CASE WHEN b.state ? 'admin'
            THEN (b.state - '{hidden,hidden_by}'::text[]) || m.field
            ELSE b.state END AS state
    FROM joppa.board_cache b JOIN meta m ON m.workspace = b.workspace
)
UPDATE joppa.board_cache b
SET first_view = n.first_view, state = n.state
FROM boards n
WHERE b.workspace = n.workspace AND (b.first_view, b.state) IS DISTINCT FROM (n.first_view, n.state);
