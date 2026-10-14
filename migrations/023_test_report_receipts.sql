-- Each identical repeat of a clean report upload is one more receipt of the stored
-- report; the report and its results stay as they are. Rows are only appended.
CREATE TABLE IF NOT EXISTS joppa.test_report_receipts (
    report_id bigint NOT NULL REFERENCES joppa.test_reports(id),
    received_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX IF NOT EXISTS test_report_receipts_by_report
    ON joppa.test_report_receipts (report_id, received_at);

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'test_report_receipts_worm' AND tgrelid = 'joppa.test_report_receipts'::regclass) THEN
        CREATE TRIGGER test_report_receipts_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.test_report_receipts FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
