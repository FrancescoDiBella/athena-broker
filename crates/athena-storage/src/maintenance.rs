//! Bounded, concurrent-safe retention. Pending work is never a retention candidate.
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionConfig {
    pub interval_sec: u64,
    pub batch_size: i64,
    pub delivered_seconds: u64,
    pub processed_events_seconds: u64,
    pub dead_seconds: u64,
    pub history_seconds: u64,
}
impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            interval_sec: 60,
            batch_size: 1000,
            delivered_seconds: 604800,
            processed_events_seconds: 604800,
            dead_seconds: 0,
            history_seconds: 0,
        }
    }
}
impl RetentionConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=86400).contains(&self.interval_sec)
            || !(1..=10000).contains(&self.batch_size)
            || [
                self.delivered_seconds,
                self.processed_events_seconds,
                self.dead_seconds,
                self.history_seconds,
            ]
            .iter()
            .any(|v| *v > 315360000)
        {
            return Err("Invalid retention settings: interval 1..86400, batch 1..10000, ages <= 10 years; 0 disables deletion".into());
        }
        Ok(())
    }
}
#[derive(Debug, Default)]
pub struct CleanupResult {
    pub jobs: u64,
    pub events: u64,
    pub history: u64,
}

pub async fn cleanup_once(
    pool: &PgPool,
    config: &RetentionConfig,
) -> Result<CleanupResult, sqlx::Error> {
    let mut result = CleanupResult::default();
    // One bounded statement per category, independent of backlog size. Multiple replicas
    // skip each other's locks; foreign keys protect events while fan-out jobs exist.
    for (status, seconds) in [
        ("delivered", config.delivered_seconds),
        ("dead", config.dead_seconds),
    ] {
        if seconds == 0 {
            continue;
        }
        result.jobs += sqlx::query("WITH expired AS (SELECT id FROM notification_jobs WHERE status=$1 AND status<>'pending' AND completed_at<statement_timestamp()-make_interval(secs=>$2::double precision) AND lease_token IS NULL ORDER BY completed_at,id LIMIT $3 FOR UPDATE SKIP LOCKED) DELETE FROM notification_jobs j USING expired x WHERE j.id=x.id")
            .bind(status).bind(seconds as f64).bind(config.batch_size).execute(pool).await?.rows_affected();
    }
    for (dead, seconds) in [
        (false, config.processed_events_seconds),
        (true, config.dead_seconds),
    ] {
        if seconds == 0 {
            continue;
        }
        result.events += sqlx::query("WITH expired AS (SELECT e.id FROM entity_events e WHERE e.processed_at<statement_timestamp()-make_interval(secs=>$1::double precision) AND (e.last_error IS NOT NULL)=$2 AND NOT EXISTS (SELECT 1 FROM notification_jobs j WHERE j.event_id=e.id) ORDER BY e.processed_at,e.id LIMIT $3 FOR UPDATE OF e SKIP LOCKED) DELETE FROM entity_events e USING expired x WHERE e.id=x.id")
            .bind(seconds as f64).bind(dead).bind(config.batch_size).execute(pool).await?.rows_affected();
    }
    if config.history_seconds > 0 {
        result.history = sqlx::query("WITH expired AS (SELECT entity_id,attribute_id,instance_id FROM entity_temporal WHERE recorded_at<statement_timestamp()-make_interval(secs=>$1::double precision) ORDER BY recorded_at LIMIT $2 FOR UPDATE SKIP LOCKED) DELETE FROM entity_temporal t USING expired x WHERE t.entity_id=x.entity_id AND t.attribute_id=x.attribute_id AND t.instance_id=x.instance_id")
            .bind(config.history_seconds as f64).bind(config.batch_size).execute(pool).await?.rows_affected();
    }
    Ok(result)
}
