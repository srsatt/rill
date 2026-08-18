CREATE TABLE document_links (
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    normalized_url TEXT NOT NULL,
    original_url TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (length(relation) BETWEEN 1 AND 32),
    title TEXT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (document_id, normalized_url)
) STRICT;
