CREATE TABLE preference_models (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    model_version INTEGER NOT NULL,
    feature_version INTEGER NOT NULL,
    embedding_provider TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    embedding_version TEXT NOT NULL,
    coefficients_json TEXT NOT NULL CHECK (json_valid(coefficients_json)),
    sample_count INTEGER NOT NULL CHECK (sample_count > 1),
    positive_count INTEGER NOT NULL CHECK (positive_count > 0),
    negative_count INTEGER NOT NULL CHECK (negative_count > 0),
    trained_event_count INTEGER NOT NULL CHECK (trained_event_count >= sample_count),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
