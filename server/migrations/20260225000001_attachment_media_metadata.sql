-- Add media processing metadata to file_attachments
ALTER TABLE file_attachments
    ADD COLUMN width INTEGER,
    ADD COLUMN height INTEGER,
    ADD COLUMN blurhash VARCHAR(100),
    ADD COLUMN thumbnail_s3_key TEXT,
    ADD COLUMN medium_s3_key TEXT,
    ADD COLUMN processing_status VARCHAR(20) NOT NULL DEFAULT 'skipped'
        CHECK (processing_status IN ('pending', 'processed', 'partial', 'failed', 'skipped'));

-- Pre-existing rows get 'skipped' (never processed). New uploads always pass status explicitly.

CREATE INDEX idx_file_attachments_processing_status
    ON file_attachments (processing_status)
    WHERE processing_status = 'pending';
