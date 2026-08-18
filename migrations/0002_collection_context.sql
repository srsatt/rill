ALTER TABLE collection_entries ADD COLUMN commentary TEXT;
ALTER TABLE collection_entries ADD COLUMN extraction_method TEXT NOT NULL DEFAULT 'deterministic';
ALTER TABLE collection_entries ADD COLUMN confidence REAL NOT NULL DEFAULT 0.0
    CHECK (confidence >= 0 AND confidence <= 1);
