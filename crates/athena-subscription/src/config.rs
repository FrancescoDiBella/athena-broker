use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub worker_threads: usize,
    pub poll_interval_ms: u64,
    pub event_batch_size: i64,
    pub request_timeout_sec: u64,
    pub lease_duration_sec: u64,
    pub max_attempts: i32,
    pub retry_max_delay_sec: u64,
    pub scheduler_max_entities: usize,
    pub scheduler_max_payload_bytes: usize,
    pub scheduler_pending_job_limit: i64,
    pub mqtt_ca_file: Option<String>,
}
impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            poll_interval_ms: 250,
            event_batch_size: 100,
            request_timeout_sec: 10,
            lease_duration_sec: 90,
            max_attempts: 12,
            retry_max_delay_sec: 300,
            scheduler_max_entities: 10_000,
            scheduler_max_payload_bytes: 4 * 1024 * 1024,
            scheduler_pending_job_limit: 100_000,
            mqtt_ca_file: None,
        }
    }
}
impl EngineConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=64).contains(&self.worker_threads)
            || !(10..=60_000).contains(&self.poll_interval_ms)
            || !(1..=1000).contains(&self.event_batch_size)
            || !(1..=300).contains(&self.request_timeout_sec)
            || !(1..=100).contains(&self.max_attempts)
            || !(1..=86400).contains(&self.retry_max_delay_sec)
            || self.lease_duration_sec < self.request_timeout_sec + 5
            || self.lease_duration_sec > 3600
            || !(1..=100_000).contains(&self.scheduler_max_entities)
            || !(1024..=64 * 1024 * 1024).contains(&self.scheduler_max_payload_bytes)
            || !(1..=10_000_000).contains(&self.scheduler_pending_job_limit)
        {
            return Err("Invalid subscriptions configuration: check workers, batch size, retries and lease > request timeout + 5s".into());
        }
        Ok(())
    }
}
