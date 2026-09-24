-- 0002_temporal_hypertable.sql
-- Temporal evolution storage for NGSI-LD

CREATE TABLE IF NOT EXISTS entity_temporal (
    entity_id TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    attribute_id TEXT NOT NULL,
    observed_at TIMESTAMPTZ NOT NULL,
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    value_numeric DOUBLE PRECISION,
    value_text TEXT,
    value_json JSONB,
    geom GEOMETRY(Geometry, 4326),
    dataset_id TEXT,
    PRIMARY KEY (entity_id, attribute_id, observed_at)
);

CREATE INDEX IF NOT EXISTS idx_temporal_query ON entity_temporal (entity_id, attribute_id, observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_temporal_time ON entity_temporal (observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_temporal_geom ON entity_temporal USING GIST (geom);
