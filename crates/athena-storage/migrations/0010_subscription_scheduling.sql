ALTER TABLE subscriptions ALTER COLUMN time_interval TYPE DOUBLE PRECISION;

CREATE TABLE subscription_schedules (
    subscription_id TEXT PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE,
    next_run_at TIMESTAMPTZ NOT NULL,
    retry_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX idx_schedules_due ON subscription_schedules (retry_at, next_run_at);

-- Client PATCH updates modified_at; notification accounting does not reset the clock.
CREATE FUNCTION athena_sync_schedule() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.time_interval IS NOT NULL AND NEW.time_interval > 0 THEN
        INSERT INTO subscription_schedules(subscription_id,next_run_at)
        VALUES(NEW.id,clock_timestamp()+make_interval(secs=>NEW.time_interval))
        ON CONFLICT(subscription_id) DO UPDATE SET
            next_run_at=EXCLUDED.next_run_at,retry_at=clock_timestamp(),attempts=0,last_error=NULL;
    ELSE
        DELETE FROM subscription_schedules WHERE subscription_id=NEW.id;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER subscriptions_schedule AFTER INSERT OR UPDATE OF modified_at ON subscriptions
FOR EACH ROW EXECUTE FUNCTION athena_sync_schedule();
INSERT INTO subscription_schedules(subscription_id,next_run_at)
SELECT id,clock_timestamp()+make_interval(secs=>time_interval) FROM subscriptions WHERE time_interval>0;

-- Scheduled jobs have no entity mutation. They share the event sequence for ordering,
-- without inventing entity events or history records.
ALTER TABLE notification_jobs ALTER COLUMN event_id DROP NOT NULL;
ALTER TABLE notification_jobs ALTER COLUMN entity_id DROP NOT NULL;
ALTER TABLE notification_jobs ADD COLUMN scheduled_at TIMESTAMPTZ;
ALTER TABLE notification_jobs ADD COLUMN ordering_id BIGINT;
UPDATE notification_jobs SET ordering_id=event_id;
ALTER TABLE notification_jobs ALTER COLUMN ordering_id SET NOT NULL;
ALTER TABLE notification_jobs ALTER COLUMN ordering_id SET DEFAULT nextval('entity_events_id_seq');
ALTER TABLE notification_jobs ADD CONSTRAINT job_origin CHECK ((event_id IS NULL) = (scheduled_at IS NOT NULL));
CREATE UNIQUE INDEX idx_jobs_schedule_unique ON notification_jobs(subscription_id,scheduled_at) WHERE scheduled_at IS NOT NULL;
CREATE INDEX idx_jobs_schedule_pending ON notification_jobs(subscription_id) WHERE status='pending' AND scheduled_at IS NOT NULL;
DROP INDEX notification_jobs_endpoint_order_idx;
DROP INDEX notification_jobs_subscription_order_idx;
DROP INDEX notification_jobs_ready_order_idx;
CREATE INDEX idx_jobs_endpoint_order ON notification_jobs(endpoint,ordering_id,id) WHERE status='pending';
CREATE INDEX idx_jobs_subscription_order ON notification_jobs(subscription_id,ordering_id,id) WHERE status='pending';
CREATE INDEX idx_jobs_ready ON notification_jobs(available_at,ordering_id,id) WHERE status='pending';
