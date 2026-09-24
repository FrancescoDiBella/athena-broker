-- 0001_init_schema.sql
-- Initialize core NGSI-LD tables with PostGIS support

CREATE EXTENSION IF NOT EXISTS postgis;

-- Core entities table
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    types TEXT[] NOT NULL DEFAULT '{}',
    attrs JSONB NOT NULL DEFAULT '{}'::jsonb,
    location GEOMETRY(Geometry, 4326),
    scope TEXT[] DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Spatial GIST index for ultra-fast geospatial search
CREATE INDEX IF NOT EXISTS idx_entities_location ON entities USING GIST (location);

-- JSONB Path index for high performance nested attribute filters
CREATE INDEX IF NOT EXISTS idx_entities_attrs_path ON entities USING GIN (attrs jsonb_path_ops);

-- B-Tree indexes for fast type and timestamp queries
CREATE INDEX IF NOT EXISTS idx_entities_type ON entities (type);
CREATE INDEX IF NOT EXISTS idx_entities_modified_at ON entities (modified_at DESC);
CREATE INDEX IF NOT EXISTS idx_entities_created_at ON entities (created_at DESC);

-- Subscriptions table
CREATE TABLE IF NOT EXISTS subscriptions (
    id TEXT PRIMARY KEY,
    subscription_name TEXT,
    description TEXT,
    entities JSONB NOT NULL,
    watched_attributes TEXT[] DEFAULT '{}',
    q TEXT,
    geo_q JSONB,
    notification JSONB NOT NULL,
    throttling BIGINT,
    time_interval BIGINT,
    last_notification TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_subscriptions_expires_at ON subscriptions (expires_at) WHERE expires_at IS NOT NULL;
