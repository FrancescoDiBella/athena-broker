-- 0003_csource_registrations.sql
-- Context Source Registration (CSR) and Distributed Federation

CREATE TABLE IF NOT EXISTS csource_registrations (
    id TEXT PRIMARY KEY,
    registration_name TEXT,
    description TEXT,
    information JSONB NOT NULL DEFAULT '[]'::jsonb,
    endpoint TEXT NOT NULL,
    context_source_info JSONB,
    location GEOMETRY(Geometry, 4326),
    observation_space GEOMETRY(Geometry, 4326),
    operation_space GEOMETRY(Geometry, 4326),
    expires_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_csource_location ON csource_registrations USING GIST (location);
CREATE INDEX IF NOT EXISTS idx_csource_observation_space ON csource_registrations USING GIST (observation_space);
CREATE INDEX IF NOT EXISTS idx_csource_operation_space ON csource_registrations USING GIST (operation_space);
CREATE INDEX IF NOT EXISTS idx_csource_status ON csource_registrations (status);
CREATE INDEX IF NOT EXISTS idx_csource_information ON csource_registrations USING GIN (information);
CREATE INDEX IF NOT EXISTS idx_csource_expires_at ON csource_registrations (expires_at) WHERE expires_at IS NOT NULL;
