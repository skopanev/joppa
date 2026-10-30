-- Attachment bytes live in DigitalOcean Spaces under files/<sha256> (src/spaces.rs).
-- The journal keeps the attachment record with the hash. PostgreSQL keeps no file bytes.
DROP TABLE IF EXISTS joppa.blobs;
