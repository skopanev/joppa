-- A Task has no purpose. The queue keeps no purpose column.
ALTER TABLE joppa.task_queue DROP COLUMN IF EXISTS purpose;
-- A criterion may move to another requirement. The journal replay checks that each criterion
-- is in the current revision of one requirement, so no table keeps a first owner.
DROP TABLE IF EXISTS joppa.ac_owners;
