-- Claim one head job without scanning/sorting the entire endpoint backlog.
CREATE INDEX notification_jobs_endpoint_order_idx
ON notification_jobs(endpoint,event_id,id) WHERE status='pending';
CREATE INDEX notification_jobs_ready_order_idx
ON notification_jobs(available_at,event_id,id) WHERE status='pending';
DROP INDEX idx_jobs_ready;
