-- Merge by attribute + dataset identity under the UPDATE row lock.
CREATE FUNCTION athena_instances(value JSONB) RETURNS JSONB LANGUAGE sql IMMUTABLE AS $$
 SELECT CASE WHEN value IS NULL THEN '[]'::jsonb WHEN jsonb_typeof(value)='array' THEN value ELSE jsonb_build_array(value) END;
$$;
CREATE FUNCTION athena_merge_attrs(previous JSONB, incoming JSONB, overwrite BOOLEAN) RETURNS JSONB LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE k TEXT; v JSONB; result JSONB := previous; merged JSONB;
BEGIN
 FOR k,v IN SELECT * FROM jsonb_each(incoming) LOOP
  IF k='@context' THEN CONTINUE; END IF;
  SELECT COALESCE(jsonb_agg(value ORDER BY dataset NULLS FIRST), '[]'::jsonb) INTO merged FROM (
   SELECT DISTINCT ON (value->>'datasetId') value, value->>'datasetId' AS dataset
   FROM (
    SELECT value, CASE WHEN overwrite THEN 1 ELSE 0 END AS priority FROM jsonb_array_elements(athena_instances(previous->k))
    UNION ALL
    SELECT value, CASE WHEN overwrite THEN 0 ELSE 1 END AS priority FROM jsonb_array_elements(athena_instances(v))
   ) candidates ORDER BY value->>'datasetId', priority
  ) picked;
  result := jsonb_set(result, ARRAY[k], CASE WHEN jsonb_array_length(merged)=1 THEN merged->0 ELSE merged END);
 END LOOP;
 RETURN result;
END;
$$;
-- Every mutation path derives its spatial index from the final document.
CREATE OR REPLACE FUNCTION athena_entity_revision() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE geo JSONB;
BEGIN
 NEW.revision := OLD.revision + 1;
 NEW.modified_at := statement_timestamp();
 SELECT value->'value' INTO geo FROM jsonb_array_elements(athena_instances(NEW.attrs->'location'))
 WHERE value->>'type'='GeoProperty' AND NOT value ? 'datasetId' LIMIT 1;
 NEW.location := CASE WHEN geo IS NOT NULL THEN ST_SetSRID(ST_GeomFromGeoJSON(geo::text),4326) ELSE NULL END;
 RETURN NEW;
END;
$$;

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
              ON (n.value->>'datasetId') IS NOT DISTINCT FROM (o.value->>'datasetId')
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
