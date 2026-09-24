CREATE OR REPLACE FUNCTION athena_capture_mutation() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    entity_row entities%ROWTYPE;
    previous JSONB;
    current JSONB;
    changed TEXT[];
    attribute_name TEXT;
    attribute_value JSONB;
    attribute_instance JSONB;
    iid TEXT;
    pair RECORD;
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
        FOR pair IN
            SELECT COALESCE(n.value,o.value) AS instance, n.value IS NULL AS deleted
            FROM jsonb_array_elements(athena_instances(current->attribute_name)) n
            FULL JOIN jsonb_array_elements(athena_instances(previous->attribute_name)) o
              ON COALESCE(n.value->>'datasetId','') = COALESCE(o.value->>'datasetId','')
            WHERE n.value IS DISTINCT FROM o.value
        LOOP
            attribute_instance := pair.instance;
            IF jsonb_typeof(attribute_instance) <> 'object' THEN CONTINUE; END IF;
            iid := 'urn:ngsi-ld:Instance:' || gen_random_uuid()::text;
            instance_observed := (attribute_instance->>'observedAt')::timestamptz;
            attribute_instance := attribute_instance || jsonb_build_object('instanceId', iid,
                'createdAt', COALESCE((attribute_instance->>'createdAt')::timestamptz, timestamp_now), 'modifiedAt', timestamp_now);
            IF pair.deleted THEN
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
            'createdAt', entity_row.created_at, 'modifiedAt', entity_row.modified_at) || CASE WHEN entity_row.scope IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('scope',entity_row.scope) END,
        CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE OLD.attrs || jsonb_build_object('id', OLD.id, 'type', OLD.types) END,
        changed);
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END;
$$;
