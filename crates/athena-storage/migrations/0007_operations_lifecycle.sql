-- Preserve contexts for subscription retrieval and durable delivery snapshots.
ALTER TABLE subscriptions ADD COLUMN context JSONB;
-- Retention uses completion/ingestion time, never a device's observedAt timestamp.
ALTER TABLE notification_jobs ADD COLUMN completed_at TIMESTAMPTZ;
UPDATE notification_jobs SET completed_at=COALESCE(delivered_at,clock_timestamp()) WHERE status<>'pending';
CREATE FUNCTION athena_job_completion() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status IS DISTINCT FROM OLD.status THEN
        NEW.completed_at := CASE WHEN NEW.status='pending' THEN NULL ELSE clock_timestamp() END;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER athena_job_completed BEFORE UPDATE OF status ON notification_jobs
    FOR EACH ROW EXECUTE FUNCTION athena_job_completion();
CREATE INDEX notification_jobs_retention_idx ON notification_jobs(status,completed_at,id) WHERE status<>'pending';
CREATE INDEX entity_events_retention_idx ON entity_events(processed_at,id) WHERE processed_at IS NOT NULL;
ALTER TABLE entity_temporal ADD COLUMN recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp();
CREATE INDEX entity_temporal_retention_idx ON entity_temporal(recorded_at);

ALTER TABLE subscriptions ALTER COLUMN throttling TYPE DOUBLE PRECISION USING throttling::double precision;
