use crate::federation::FederationService;
use athena_jsonld::ContextResolver;
use athena_storage::{
    CsourceRepository, EntityRepository, SubscriptionRepository, TemporalRepository,
};
use athena_subscription::SubscriptionEngine;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub limits: ApiLimits,
    pub outbound_policy: athena_http::OutboundPolicy,
    pub write_slots: Arc<tokio::sync::Semaphore>,
    pub admission: Arc<tokio::sync::Mutex<(std::time::Instant, bool)>>,
    pub processor: Arc<athena_jsonld::Processor>,
    pub pool: sqlx::PgPool,
    pub metrics: Arc<crate::middleware::RequestMetrics>,
    pub entity_repo: Arc<dyn EntityRepository>,
    pub subscription_repo: Arc<dyn SubscriptionRepository>,
    pub temporal_repo: Arc<dyn TemporalRepository>,
    pub csource_repo: Arc<dyn CsourceRepository>,
    pub subscription_engine: Arc<SubscriptionEngine>,
    pub context_resolver: Arc<ContextResolver>,
    pub federation_service: Arc<FederationService>,
}

impl AppState {
    pub fn new(
        pool: sqlx::PgPool,
        entity_repo: Arc<dyn EntityRepository>,
        subscription_repo: Arc<dyn SubscriptionRepository>,
        temporal_repo: Arc<dyn TemporalRepository>,
        csource_repo: Arc<dyn CsourceRepository>,
        subscription_engine: Arc<SubscriptionEngine>,
        context_resolver: Arc<ContextResolver>,
    ) -> Self {
        Self {
            pool,
            write_slots: Arc::new(tokio::sync::Semaphore::new(
                ApiLimits::default().max_in_flight_writes,
            )),
            limits: ApiLimits::default(),
            outbound_policy: athena_http::OutboundPolicy::default(),
            admission: Arc::new(tokio::sync::Mutex::new((
                std::time::Instant::now() - std::time::Duration::from_secs(10),
                true,
            ))),
            processor: Arc::new(athena_jsonld::Processor::default()),
            metrics: Arc::new(crate::middleware::RequestMetrics::default()),
            entity_repo,
            subscription_repo,
            temporal_repo,
            csource_repo,
            subscription_engine,
            context_resolver,
            federation_service: Arc::new(FederationService::new()),
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApiLimits {
    pub max_in_flight_writes: usize,
    pub max_pending_events: i64,
    pub max_body_bytes: usize,
}
impl Default for ApiLimits {
    fn default() -> Self {
        Self {
            max_in_flight_writes: 128,
            max_pending_events: 100_000,
            max_body_bytes: 4 * 1024 * 1024,
        }
    }
}
impl ApiLimits {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=4096).contains(&self.max_in_flight_writes)
            || !(1..=10_000_000).contains(&self.max_pending_events)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_body_bytes)
        {
            return Err("Invalid API concurrency, queue or body limit".into());
        }
        Ok(())
    }
}
impl AppState {
    pub fn configure(
        mut self,
        limits: ApiLimits,
        policy: athena_http::OutboundPolicy,
        processor: Arc<athena_jsonld::Processor>,
    ) -> Self {
        self.write_slots = Arc::new(tokio::sync::Semaphore::new(limits.max_in_flight_writes));
        self.limits = limits;
        self.outbound_policy = policy;
        self.processor = processor.clone();
        self.federation_service = Arc::new(FederationService::with_policy(policy, processor));
        self
    }
}
