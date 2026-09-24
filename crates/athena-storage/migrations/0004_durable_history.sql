-- Existing installations had no migration journal. The first three idempotent
-- migrations can be adopted before applying this additive migration.
ALTER TABLE entities ADD COLUMN revision BIGINT NOT NULL DEFAULT 1;
ALTER TABLE entity_temporal ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE entity_temporal ADD COLUMN instance_id TEXT NOT NULL DEFAULT ('urn:ngsi-ld:Instance:' || gen_random_uuid()::text);
ALTER TABLE entity_temporal ADD COLUMN attribute_type TEXT NOT NULL DEFAULT 'Property';
ALTER TABLE entity_temporal ADD COLUMN instance JSONB;
ALTER TABLE entity_temporal DROP CONSTRAINT entity_temporal_pkey;
ALTER TABLE entity_temporal ALTER COLUMN observed_at DROP NOT NULL;
ALTER TABLE entity_temporal ADD PRIMARY KEY (entity_id, attribute_id, instance_id);
CREATE INDEX idx_temporal_dataset ON entity_temporal (entity_id, attribute_id, dataset_id, observed_at DESC);

CREATE TABLE temporal_entities (
    id TEXT PRIMARY KEY,
    types TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO temporal_entities (id, types, created_at, modified_at)
SELECT entity_id, ARRAY[min(entity_type)], min(created_at), max(modified_at)
FROM entity_temporal GROUP BY entity_id;

UPDATE entity_temporal SET instance = jsonb_strip_nulls(jsonb_build_object(
    'type', attribute_type, 'value', value_json, 'datasetId', dataset_id,
    'observedAt', observed_at, 'createdAt', created_at,
    'modifiedAt', modified_at, 'instanceId', instance_id));

CREATE TABLE entity_events (
    id BIGSERIAL PRIMARY KEY,
    entity_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    operation TEXT NOT NULL,
    entity JSONB NOT NULL,
    previous_entity JSONB,
    changed_attrs TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    processed_at TIMESTAMPTZ,
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX idx_events_ready ON entity_events (id) WHERE processed_at IS NULL;
CREATE INDEX idx_events_entity_pending ON entity_events (entity_id, id) WHERE processed_at IS NULL;

CREATE TABLE notification_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id BIGINT NOT NULL REFERENCES entity_events(id),
    subscription_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    subscription JSONB NOT NULL,
    notification JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered','dead')),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    last_error TEXT,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (event_id, subscription_id)
);
CREATE INDEX idx_jobs_ready ON notification_jobs (available_at, event_id) WHERE status = 'pending';
CREATE INDEX idx_jobs_endpoint ON notification_jobs (endpoint, lease_until) WHERE status = 'pending';
CREATE INDEX idx_jobs_order ON notification_jobs (subscription_id, entity_id, event_id) WHERE status = 'pending';

-- Runs in the caller's transaction, so all repository mutation paths have the
-- same atomic history/outbox guarantees, including batch and deletion.
CREATE FUNCTION athena_capture_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    entity_row entities%ROWTYPE;
    previous JSONB;
    current JSONB;
    changed TEXT[];
    attribute_name TEXT;
    attribute_value JSONB;
    attribute_instance JSONB;
    iid TEXT;
    timestamp_now TIMESTAMPTZ := statement_timestamp();
    instance_observed TIMESTAMPTZ;
BEGIN
    IF TG_OP = 'DELETE' THEN entity_row := OLD; ELSE entity_row := NEW; END IF;
    previous := CASE WHEN TG_OP = 'INSERT' THEN '{}'::jsonb ELSE OLD.attrs END;
    current := CASE WHEN TG_OP = 'DELETE' THEN '{}'::jsonb ELSE NEW.attrs END;
    SELECT array_agg(k ORDER BY k) INTO changed FROM (
        SELECT jsonb_object_keys(previous) AS k UNION SELECT jsonb_object_keys(current) AS k
    ) keys WHERE previous->k IS DISTINCT FROM current->k;
    changed := COALESCE(changed, ARRAY[]::text[]);
    IF TG_OP = 'UPDATE' AND cardinality(changed) = 0 AND OLD.types = NEW.types AND OLD.scope IS NOT DISTINCT FROM NEW.scope THEN
        RETURN NEW;
    END IF;

    INSERT INTO temporal_entities (id, types, created_at, modified_at)
    VALUES (entity_row.id, entity_row.types, entity_row.created_at, timestamp_now)
    ON CONFLICT (id) DO UPDATE SET types = EXCLUDED.types, modified_at = EXCLUDED.modified_at;

    FOREACH attribute_name IN ARRAY changed LOOP
        IF attribute_name LIKE '@%' THEN CONTINUE; END IF;
        attribute_value := COALESCE(current->attribute_name, previous->attribute_name);
        IF jsonb_typeof(attribute_value) = 'object' THEN attribute_value := jsonb_build_array(attribute_value); END IF;
        IF jsonb_typeof(attribute_value) <> 'array' THEN CONTINUE; END IF;
        FOR attribute_instance IN SELECT value FROM jsonb_array_elements(attribute_value) LOOP
            IF jsonb_typeof(attribute_instance) <> 'object' THEN CONTINUE; END IF;
            iid := 'urn:ngsi-ld:Instance:' || gen_random_uuid()::text;
            instance_observed := (attribute_instance->>'observedAt')::timestamptz;
            attribute_instance := attribute_instance || jsonb_build_object('instanceId', iid,
                'createdAt', COALESCE((attribute_instance->>'createdAt')::timestamptz, timestamp_now), 'modifiedAt', timestamp_now);
            IF NOT current ? attribute_name THEN
                attribute_instance := attribute_instance || jsonb_build_object('deletedAt', timestamp_now);
            END IF;
            INSERT INTO entity_temporal (entity_id, entity_type, attribute_id, observed_at,
                created_at, modified_at, value_numeric, value_text, value_json, dataset_id, instance_id, attribute_type, instance)
            VALUES (entity_row.id, entity_row.type, attribute_name, instance_observed,
                (attribute_instance->>'createdAt')::timestamptz, timestamp_now,
                CASE WHEN jsonb_typeof(attribute_instance->'value') = 'number' THEN (attribute_instance->>'value')::double precision END,
                CASE WHEN jsonb_typeof(attribute_instance->'value') = 'string' THEN attribute_instance->>'value' END,
                attribute_instance->'value', attribute_instance->>'datasetId', iid,
                attribute_instance->>'type', attribute_instance);
        END LOOP;
    END LOOP;

    INSERT INTO entity_events(entity_id, revision, operation, entity, previous_entity, changed_attrs)
    VALUES (entity_row.id, entity_row.revision, lower(TG_OP),
        entity_row.attrs || jsonb_build_object('id', entity_row.id, 'type', entity_row.types,
            'createdAt', entity_row.created_at, 'modifiedAt', entity_row.modified_at),
        CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.attrs || jsonb_build_object('id', OLD.id, 'type', OLD.types) END,
        changed);
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$;
CREATE TRIGGER entities_capture_mutation AFTER INSERT OR UPDATE OR DELETE ON entities
FOR EACH ROW EXECUTE FUNCTION athena_capture_mutation();

CREATE FUNCTION athena_entity_revision() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.revision := OLD.revision + 1;
    NEW.modified_at := statement_timestamp();
    RETURN NEW;
END;
$$;
CREATE TRIGGER entities_revision BEFORE UPDATE ON entities
FOR EACH ROW EXECUTE FUNCTION athena_entity_revision();
