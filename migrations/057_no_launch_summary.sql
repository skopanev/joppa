-- The board cache keeps no launch summary, and the state keeps no acceptance.
ALTER TABLE joppa.board_cache DROP COLUMN IF EXISTS launch;
DELETE FROM joppa.state_entries WHERE section = 'acceptances';
