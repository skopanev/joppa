-- The readiness marks were removed (assess). The state keeps no mark, and the replay rules
-- of the marks have no epoch.
DELETE FROM joppa.state_entries WHERE section = 'signal_assessments';
DELETE FROM joppa.replay_epochs WHERE rule IN ('keep_assessments', 'own_mark_reset');
