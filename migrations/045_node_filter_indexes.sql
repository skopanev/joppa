-- The node filters of every read (src/node_filter.rs) use these indexes. Each expression is
-- the exact expression of the filter clause, so the planner can use it.
CREATE INDEX IF NOT EXISTS objects_owner ON joppa.objects(workspace,owner) WHERE present;
CREATE INDEX IF NOT EXISTS objects_tone ON joppa.objects(workspace,(COALESCE(rollup->'signal'->>'tone',signal->>'tone'))) WHERE present;
