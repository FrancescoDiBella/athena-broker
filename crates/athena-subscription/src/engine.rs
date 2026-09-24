//! PostgreSQL is the durable queue. In-process notifications only reduce poll latency.
use crate::EngineConfig;
use crate::{dispatcher::NotificationDispatcher, matcher::SubscriptionIndex};
use athena_http::{OutboundClient, OutboundPolicy};
use athena_model::{Entity, Notification, Subscription};
use athena_storage::SubscriptionRepository;
use serde_json::Value;
use sqlx::{PgPool, Row};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{watch, Mutex, Notify},
    task::JoinHandle,
};
use uuid::Uuid;

pub struct SubscriptionEngine {
    wake: Arc<Notify>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    stop: watch::Sender<bool>,
}

impl Drop for SubscriptionEngine {
    fn drop(&mut self) {
        // Claimed deliveries remain recoverable after the lease expires.
        for task in self.tasks.get_mut().iter() {
            task.abort();
        }
    }
}

impl SubscriptionEngine {
    pub fn new(repo: Arc<dyn SubscriptionRepository>, pool: PgPool, worker_threads: usize) -> Self {
        Self::with_config(
            repo,
            pool,
            EngineConfig {
                worker_threads,
                ..Default::default()
            },
            OutboundPolicy::default(),
        )
        .expect("Valid notification configuration")
    }

    pub fn with_config(
        repo: Arc<dyn SubscriptionRepository>,
        pool: PgPool,
        config: EngineConfig,
        policy: OutboundPolicy,
    ) -> Result<Self, String> {
        config.validate()?;
        let mqtt = crate::mqtt::MqttClient::from_ca_file(config.mqtt_ca_file.as_deref())?;
        let client =
            OutboundClient::with_timeout(policy, Duration::from_secs(config.request_timeout_sec))
                .map_err(err)?;
        let wake = Arc::new(Notify::new());
        let (stop, stopped) = watch::channel(false);
        let mut tasks = Vec::new();
        let p = pool.clone();
        let r = repo.clone();
        let n = wake.clone();
        let mut stop_rx = stopped.clone();
        let cfg = config.clone();
        tasks.push(tokio::spawn(async move {
            while !*stop_rx.borrow() {
                match materialize_events_with_config(&p, &r, &cfg).await {
                    Ok(n) if n > 0 => continue,
                    Ok(_) => {},
                    Err(e) => tracing::error!("Outbox matching failed; transaction remains pending: {e}"),
                }
                tokio::select! { _ = stop_rx.changed() => {}, _ = n.notified() => {}, _ = tokio::time::sleep(Duration::from_millis(cfg.poll_interval_ms)) => {} }
            }
        }));
        let p = pool.clone();
        let cfg = config.clone();
        let mut stop_rx = stopped.clone();
        tasks.push(tokio::spawn(async move {
            while !*stop_rx.borrow() {
                match crate::scheduler::materialize_schedule(&p, &cfg).await {
                    Ok(true) => continue,
                    Ok(false) => {},
                    Err(error) => tracing::error!(%error, "Scheduler transaction failed; schedule remains pending"),
                }
                tokio::select! { _ = stop_rx.changed() => {}, _ = tokio::time::sleep(Duration::from_millis(cfg.poll_interval_ms)) => {} }
            }
        }));
        for _ in 0..config.worker_threads {
            let p = pool.clone();
            let r = repo.clone();
            let c = client.clone();
            let mqtt = mqtt.clone();
            let cfg = config.clone();
            let mut stop_rx = stopped.clone();
            tasks.push(tokio::spawn(async move {
                while !*stop_rx.borrow() {
                    match deliver_one_with_clients(&p, &r, &c, &cfg, &mqtt).await {
                        Ok(true) => continue,
                        Ok(false) => {},
                        Err(e) => tracing::error!("Delivery worker failed; lease will recover: {e}"),
                    }
                    tokio::select! { _ = stop_rx.changed() => {}, _ = tokio::time::sleep(Duration::from_millis(cfg.poll_interval_ms)) => {} }
                }
            }));
        }
        Ok(Self {
            wake,
            tasks: Mutex::new(tasks),
            stop,
        })
    }

    /// Stop claiming work, finish in-flight transactions/deliveries within the deadline.
    /// On deadline, abort and leave claims recoverable by lease expiry.
    pub fn stop_claiming(&self) {
        self.stop.send_replace(true);
    }

    pub async fn is_healthy(&self) -> bool {
        if *self.stop.borrow() {
            return false;
        }
        let tasks = self.tasks.lock().await;
        !tasks.is_empty() && tasks.iter().all(|task| !task.is_finished())
    }

    pub async fn shutdown(&self, grace: Duration) -> bool {
        self.stop_claiming();
        let mut tasks = self.tasks.lock().await;
        let deadline = tokio::time::Instant::now() + grace;
        let mut drained = true;
        for task in tasks.iter_mut() {
            match tokio::time::timeout_at(deadline, &mut *task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => drained = false,
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    drained = false;
                }
            }
        }
        for task in tasks.iter_mut().filter(|t| !t.is_finished()) {
            task.abort();
        }
        tasks.clear();
        drained
    }

    pub async fn notify_mutation(&self, _entity: Entity, _mutated_attrs: Vec<String>) {
        self.wake.notify_one();
    }
}

/// Locks, matching and job insertion form one transaction: no lost fan-out on crash.
pub async fn materialize_events(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
) -> Result<usize, String> {
    materialize_events_with_config(pool, repo, &EngineConfig::default()).await
}

pub async fn materialize_events_with_config(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
    config: &EngineConfig,
) -> Result<usize, String> {
    let mut tx = pool.begin().await.map_err(err)?;
    let rows = sqlx::query("SELECT e.id,e.entity,e.changed_attrs,e.created_at FROM entity_events e WHERE e.processed_at IS NULL AND (e.lease_until IS NULL OR e.lease_until<statement_timestamp()) AND NOT EXISTS (SELECT 1 FROM entity_events older WHERE older.entity_id=e.entity_id AND older.id<e.id AND older.processed_at IS NULL) ORDER BY e.id LIMIT $1 FOR UPDATE OF e SKIP LOCKED")
        .bind(config.event_batch_size).fetch_all(&mut *tx).await.map_err(err)?;
    if rows.is_empty() {
        return Ok(0);
    }
    // One repository read per batch, rather than per event.
    let subs = SubscriptionIndex::new(repo.get_active_subscriptions().await.map_err(err)?);
    for row in &rows {
        let id: i64 = row.try_get("id").map_err(err)?;
        sqlx::query("SAVEPOINT event_item")
            .execute(&mut *tx)
            .await
            .map_err(err)?;
        let outcome:Result<(),String>=async {
        let value: Value = row.try_get("entity").map_err(err)?;
        let attrs: Vec<String> = row.try_get("changed_attrs").map_err(err)?;
        let created: chrono::DateTime<chrono::Utc> = row.try_get("created_at").map_err(err)?;
        let entity = Entity::from_json(value).map_err(err)?;
        for prepared in subs.candidates(&entity) {
            let sub = &prepared.subscription;
            if sub.created_at.is_some_and(|t| t > created) || sub.time_interval.is_some() {
                continue;
            }
            if !prepared.matches(&entity, &attrs) {
                continue;
            }
            if let Some(geo) = &sub.geo_q {
                if !matches_geo(pool, geo, &entity).await? {
                    continue;
                }
            }
            let payload = Notification::new(
                &sub.id,
                vec![NotificationDispatcher::build_notification_data(
                    sub, &entity,
                )],
            );
            sqlx::query("INSERT INTO notification_jobs(event_id,subscription_id,entity_id,endpoint,subscription,notification,ordering_id) VALUES($1,$2,$3,$4,$5,$6,$1) ON CONFLICT(event_id,subscription_id) DO NOTHING")
                .bind(id).bind(&sub.id).bind(&entity.id).bind(&sub.notification.endpoint.uri)
                .bind(serde_json::to_value(sub).map_err(err)?).bind(serde_json::to_value(payload).map_err(err)?)
                .execute(&mut *tx).await.map_err(err)?;
        }
        sqlx::query("UPDATE entity_events SET processed_at=clock_timestamp(),last_error=NULL,lease_until=NULL WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            Ok(())
        }.await;
        if let Err(message) = outcome {
            sqlx::query("ROLLBACK TO SAVEPOINT event_item")
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            sqlx::query("UPDATE entity_events SET attempts=attempts+1,last_error=$2,lease_until=clock_timestamp()+interval '5 seconds',processed_at=CASE WHEN attempts+1 >= $3 THEN clock_timestamp() ELSE NULL END WHERE id=$1")
                .bind(id).bind(message).bind(config.max_attempts).execute(&mut *tx).await.map_err(err)?;
        }
        sqlx::query("RELEASE SAVEPOINT event_item")
            .execute(&mut *tx)
            .await
            .map_err(err)?;
    }
    tx.commit().await.map_err(err)?;
    Ok(rows.len())
}

/// A single head job per endpoint prevents concurrent sends to a slow receiver.
/// Requests time out before their lease; completion is fenced by a unique token.
pub async fn deliver_one(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
    client: &OutboundClient,
) -> Result<bool, String> {
    deliver_one_with_config(pool, repo, client, &EngineConfig::default()).await
}

pub async fn deliver_one_with_config(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
    client: &OutboundClient,
    config: &EngineConfig,
) -> Result<bool, String> {
    let mqtt = crate::mqtt::MqttClient::from_ca_file(config.mqtt_ca_file.as_deref())?;
    deliver_one_with_clients(pool, repo, client, config, &mqtt).await
}
async fn deliver_one_with_clients(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
    client: &OutboundClient,
    config: &EngineConfig,
    mqtt: &crate::mqtt::MqttClient,
) -> Result<bool, String> {
    // OFFSET 0 keeps predecessor probes correlated. Without it PostgreSQL may
    // flatten these into anti joins and scan the full pending queue per claim
    // when a burst arrives before autoanalyze. Keep EXISTS free to stop at its first match.
    let token = Uuid::new_v4();
    let row = sqlx::query("WITH candidate AS (SELECT j.id FROM notification_jobs j WHERE j.status='pending' AND NOT EXISTS (SELECT 1 FROM subscriptions paused WHERE paused.id=j.subscription_id AND paused.status='paused' AND (paused.expires_at IS NULL OR paused.expires_at>clock_timestamp())) AND j.available_at<=statement_timestamp() AND (j.lease_until IS NULL OR j.lease_until<statement_timestamp()) AND NOT EXISTS (SELECT 1 FROM notification_jobs older WHERE older.endpoint=j.endpoint AND older.status='pending' AND (older.ordering_id,older.id)<(j.ordering_id,j.id) AND (older.lease_until>statement_timestamp() OR NOT EXISTS (SELECT 1 FROM subscriptions paused WHERE paused.id=older.subscription_id AND paused.status='paused' AND (paused.expires_at IS NULL OR paused.expires_at>statement_timestamp()))) OFFSET 0) AND NOT EXISTS (SELECT 1 FROM notification_jobs older WHERE older.subscription_id=j.subscription_id AND older.status='pending' AND (older.ordering_id,older.id)<(j.ordering_id,j.id) OFFSET 0) ORDER BY j.available_at,j.ordering_id,j.id LIMIT 1 FOR UPDATE OF j SKIP LOCKED) UPDATE notification_jobs j SET lease_token=$1,lease_until=clock_timestamp()+make_interval(secs=>$2::double precision),attempts=attempts+1 FROM candidate c WHERE j.id=c.id RETURNING j.id,j.subscription,j.notification,j.attempts")
        .bind(token).bind(config.lease_duration_sec as f64).fetch_optional(pool).await.map_err(err)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let id: Uuid = row.try_get("id").map_err(err)?;
    let attempts: i32 = row.try_get("attempts").map_err(err)?;
    let claim = ClaimedJob {
        id,
        token,
        attempts,
        row,
    };
    let outcome = tokio::time::timeout(
        Duration::from_secs(config.lease_duration_sec.saturating_sub(1)),
        deliver_claim(pool, repo, client, config, mqtt, claim),
    )
    .await;
    match outcome {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(message)) => {
            let dead = attempts >= config.max_attempts;
            sqlx::query("UPDATE notification_jobs SET status=$3,last_error=$4,available_at=clock_timestamp()+interval '5 seconds',lease_token=NULL,lease_until=NULL WHERE id=$1 AND lease_token=$2")
                .bind(id).bind(token).bind(if dead { "dead" } else { "pending" }).bind(&message).execute(pool).await.map_err(err)?;
            tracing::warn!(job_id=%id,%message,"Delivery preparation failed; retry limit applies");
            Ok(true)
        }
        Err(_) => {
            // The cancelled send can have reached the receiver. Keep the stable ID
            // and lease; another worker may retry after expiry, with at-least-once semantics.
            Err("Delivery exceeded its lease budget; claim remains recoverable".into())
        }
    }
}
struct ClaimedJob {
    id: Uuid,
    token: Uuid,
    attempts: i32,
    row: sqlx::postgres::PgRow,
}

async fn deliver_claim(
    pool: &PgPool,
    repo: &Arc<dyn SubscriptionRepository>,
    client: &OutboundClient,
    config: &EngineConfig,
    mqtt: &crate::mqtt::MqttClient,
    claim: ClaimedJob,
) -> Result<bool, String> {
    let ClaimedJob {
        id,
        token,
        attempts,
        row,
    } = claim;
    let sub: Subscription =
        serde_json::from_value(row.try_get("subscription").map_err(err)?).map_err(err)?;
    let notification: Value = row.try_get("notification").map_err(err)?;
    // Recheck after claiming to handle lifecycle updates racing with the claim.
    let live = repo.get_subscription_by_id(&sub.id).await.map_err(err)?;
    if live.as_ref().is_some_and(|s| {
        s.status == athena_model::SubscriptionStatus::Paused
            && s.expires_at.is_none_or(|t| t > chrono::Utc::now())
    }) {
        sqlx::query("UPDATE notification_jobs SET lease_token=NULL,lease_until=NULL,attempts=attempts-1,available_at=clock_timestamp()+interval '1 second' WHERE id=$1 AND lease_token=$2")
            .bind(id).bind(token).execute(pool).await.map_err(err)?;
        return Ok(true);
    }
    let active = live.as_ref().is_some_and(|s| {
        s.status == athena_model::SubscriptionStatus::Active
            && s.expires_at.is_none_or(|t| t > chrono::Utc::now())
    });
    let throttling = live.as_ref().and_then(|s| s.throttling).unwrap_or(0.0);
    if active && throttling > 0.0 {
        let deferred = sqlx::query("UPDATE notification_jobs SET available_at=s.last_notification+make_interval(secs=>$3::double precision),lease_token=NULL,lease_until=NULL,attempts=attempts-1 FROM subscriptions s WHERE notification_jobs.id=$1 AND notification_jobs.lease_token=$2 AND s.id=notification_jobs.subscription_id AND s.last_notification+make_interval(secs=>$3::double precision)>clock_timestamp()")
            .bind(id).bind(token).bind(throttling).execute(pool).await.map_err(err)?.rows_affected();
        if deferred > 0 {
            return Ok(true);
        }
    }
    let result = if active {
        NotificationDispatcher::send_with_mqtt(&sub, &notification, client, mqtt).await
    } else {
        Err("Subscription is deleted or expired".into())
    };
    let mut tx = pool.begin().await.map_err(err)?;
    match result {
        Ok(()) => {
            let changed = sqlx::query("UPDATE notification_jobs SET status='delivered',delivered_at=clock_timestamp(),lease_token=NULL,lease_until=NULL,last_error=NULL WHERE id=$1 AND lease_token=$2")
                .bind(id).bind(token).execute(&mut *tx).await.map_err(err)?.rows_affected();
            if changed > 0 {
                sqlx::query("UPDATE subscriptions SET last_notification=clock_timestamp(),notification=notification || jsonb_build_object('lastNotification',clock_timestamp(),'lastSuccess',clock_timestamp(),'timesSent',COALESCE((notification->>'timesSent')::bigint,0)+1) WHERE id=$1")
                    .bind(&sub.id).execute(&mut *tx).await.map_err(err)?;
            }
        }
        Err(message) => {
            let dead = !active || attempts >= config.max_attempts;
            // Configurable exponential backoff with deterministic per-job jitter.
            let delay = (1_i64 << attempts.clamp(0, 30)).min(config.retry_max_delay_sec as i64)
                + (id.as_u128() % 7) as i64;
            let changed = sqlx::query("UPDATE notification_jobs SET status=$3,last_error=$4,available_at=clock_timestamp()+make_interval(secs=>$5::double precision),lease_token=NULL,lease_until=NULL WHERE id=$1 AND lease_token=$2")
                .bind(id).bind(token).bind(if dead { "dead" } else { "pending" }).bind(message).bind(delay as f64)
                .execute(&mut *tx).await.map_err(err)?.rows_affected();
            if changed > 0 {
                sqlx::query("UPDATE subscriptions SET notification=notification || jsonb_build_object('lastFailure',clock_timestamp()) WHERE id=$1")
                .bind(&sub.id).execute(&mut *tx).await.map_err(err)?;
            }
        }
    }
    tx.commit().await.map_err(err)?;
    Ok(true)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn parse_subscription_geo(value: &Value) -> Result<athena_query::GeoQuery, String> {
    athena_query::GeoQueryParser::parse(
        value.get("georel").and_then(Value::as_str),
        value.get("geometry").and_then(Value::as_str),
        value.get("coordinates").map(Value::to_string).as_deref(),
        value.get("geoproperty").and_then(Value::as_str),
    )
    .map_err(err)?
    .ok_or_else(|| "Incomplete geoQ".into())
}

async fn matches_geo(pool: &PgPool, geo: &Value, entity: &Entity) -> Result<bool, String> {
    use athena_query::{SqlCompiler, SqlParam};
    let compiled = SqlCompiler::compile_geo(&parse_subscription_geo(geo)?, 3);
    let sql=format!("SELECT {} FROM (SELECT $1::jsonb AS attrs,CASE WHEN $2::text IS NULL THEN NULL ELSE ST_SetSRID(ST_GeomFromGeoJSON($2),4326) END AS location) event",compiled.where_clause);
    let location = entity
        .attributes
        .get("location")
        .and_then(|v| v.get("value"))
        .map(Value::to_string);
    let mut query = sqlx::query_scalar::<_, Option<bool>>(&sql)
        .bind(serde_json::to_value(&entity.attributes).map_err(err)?)
        .bind(location);
    for param in compiled.params {
        query = match param {
            SqlParam::String(v) => query.bind(v),
            SqlParam::Number(v) => query.bind(v),
            SqlParam::Integer(v) => query.bind(v),
            SqlParam::Boolean(v) => query.bind(v),
            SqlParam::StringList(v) => query.bind(v),
            SqlParam::NumberList(v) => query.bind(v),
        };
    }
    Ok(query.fetch_one(pool).await.map_err(err)?.unwrap_or(false))
}
