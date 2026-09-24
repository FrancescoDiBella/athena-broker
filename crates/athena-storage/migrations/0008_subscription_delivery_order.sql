-- Keep a subscription's deliveries ordered even after an endpoint update.
CREATE INDEX notification_jobs_subscription_order_idx
ON notification_jobs(subscription_id,event_id,id) WHERE status='pending';
