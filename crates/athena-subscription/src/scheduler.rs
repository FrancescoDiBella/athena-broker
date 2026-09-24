//! A schedule, its current snapshot and the durable job commit together.
//! One pending snapshot per subscription bounds outage backlog. Missed ticks are
//! coalesced; oversized snapshots fail visibly rather than losing entities.
use crate::{dispatcher::NotificationDispatcher, EngineConfig};
use athena_model::{Entity, Notification, Subscription};
use athena_query::{Parser, SqlCompiler, SqlParam};
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};

pub async fn materialize_schedule(pool: &PgPool, config: &EngineConfig) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(err)?;
    let row = sqlx::query("SELECT s.*,sc.next_run_at FROM subscriptions s JOIN subscription_schedules sc ON sc.subscription_id=s.id WHERE s.status='active' AND s.time_interval>0 AND (s.expires_at IS NULL OR s.expires_at>statement_timestamp()) AND sc.next_run_at<=statement_timestamp() AND sc.retry_at<=statement_timestamp() AND NOT EXISTS (SELECT 1 FROM notification_jobs j WHERE j.subscription_id=s.id AND j.status='pending' AND j.scheduled_at IS NOT NULL) ORDER BY sc.next_run_at,s.id LIMIT 1 FOR UPDATE OF s,sc SKIP LOCKED")
        .fetch_optional(&mut *tx).await.map_err(err)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let due: DateTime<Utc> = row.try_get("next_run_at").map_err(err)?;
    let sub = athena_storage::subscription_store::decode(row).map_err(err)?;
    // Soft global limit: bounded index scan, no full count of a large backlog.
    let full: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM notification_jobs WHERE status='pending' LIMIT 1 OFFSET $1)",
    )
    .bind(config.scheduler_pending_job_limit - 1)
    .fetch_one(&mut *tx)
    .await
    .map_err(err)?;
    if full {
        return Ok(false);
    }
    sqlx::query("SAVEPOINT schedule_snapshot")
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    match snapshot(&mut tx, &sub, config).await {
        Ok(data) => {
            if !data.is_empty() {
                let notification = Notification::new(&sub.id, data);
                sqlx::query("INSERT INTO notification_jobs(subscription_id,endpoint,subscription,notification,scheduled_at) VALUES($1,$2,$3,$4,$5) ON CONFLICT(subscription_id,scheduled_at) WHERE scheduled_at IS NOT NULL DO NOTHING")
                    .bind(&sub.id).bind(&sub.notification.endpoint.uri).bind(serde_json::to_value(&sub).map_err(err)?)
                    .bind(serde_json::to_value(notification).map_err(err)?).bind(due).execute(&mut *tx).await.map_err(err)?;
            }
            sqlx::query("UPDATE subscription_schedules SET next_run_at=clock_timestamp()+make_interval(secs=>GREATEST($2,$3)),retry_at=clock_timestamp(),attempts=0,last_error=NULL WHERE subscription_id=$1")
                .bind(&sub.id).bind(sub.time_interval.unwrap()).bind(config.poll_interval_ms as f64/1000.0).execute(&mut *tx).await.map_err(err)?;
        }
        Err(error) => {
            sqlx::query("ROLLBACK TO SAVEPOINT schedule_snapshot")
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            sqlx::query("UPDATE subscription_schedules SET attempts=attempts+1,last_error=$2,retry_at=clock_timestamp()+make_interval(secs=>LEAST(300.0,5.0*power(2.0,LEAST(attempts,6)))) WHERE subscription_id=$1")
                .bind(&sub.id).bind(&error).execute(&mut *tx).await.map_err(err)?;
            tracing::warn!(subscription_id=%sub.id,%error,"Periodic snapshot deferred");
        }
    }
    tx.commit().await.map_err(err)?;
    Ok(true)
}

async fn snapshot(
    tx: &mut Transaction<'_, Postgres>,
    sub: &Subscription,
    config: &EngineConfig,
) -> Result<Vec<Value>, String> {
    sub.validate()?;
    let mut sql = String::from(
        "SELECT id,type,types,attrs,scope,created_at,modified_at FROM entities WHERE (",
    );
    let mut params = Vec::new();
    for (n, selector) in sub.entities.iter().enumerate() {
        if n > 0 {
            sql.push_str(" OR ");
        }
        let index = params.len() + 1;
        sql.push_str(&format!(
            "((type=ANY(${index}::text[]) OR types && ${index}::text[])"
        ));
        let short = selector
            .r#type
            .strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
            .unwrap_or(&selector.r#type);
        let mut types = vec![selector.r#type.clone(), short.into()];
        if !selector.r#type.contains(':') {
            types.push(format!(
                "https://uri.etsi.org/ngsi-ld/default-context/{}",
                selector.r#type
            ));
        }
        params.push(SqlParam::StringList(types));
        if let Some(id) = &selector.id {
            sql.push_str(&format!(" AND id=${}", params.len() + 1));
            params.push(SqlParam::String(id.clone()));
        }
        if let Some(pattern) = &selector.id_pattern {
            sql.push_str(&format!(" AND id ~ ${}", params.len() + 1));
            params.push(SqlParam::String(pattern.clone()));
        }
        sql.push(')');
    }
    sql.push(')');
    if let Some(q) = &sub.q {
        let compiled = SqlCompiler::compile_q(&Parser::parse_str(q).map_err(err)?, params.len());
        sql.push_str(&format!(" AND ({})", compiled.where_clause));
        params.extend(compiled.params);
    }
    if let Some(geo) = &sub.geo_q {
        let compiled = SqlCompiler::compile_geo(
            &crate::engine::parse_subscription_geo(geo)?,
            params.len() + 1,
        );
        sql.push_str(&format!(" AND ({})", compiled.where_clause));
        params.extend(compiled.params);
    }
    sql.push_str(&format!(" ORDER BY id LIMIT ${}", params.len() + 1));
    params.push(SqlParam::Integer(config.scheduler_max_entities as i64 + 1));
    let mut query = sqlx::query(&sql);
    for param in params {
        query = match param {
            SqlParam::String(v) => query.bind(v),
            SqlParam::Number(v) => query.bind(v),
            SqlParam::Integer(v) => query.bind(v),
            SqlParam::Boolean(v) => query.bind(v),
            SqlParam::StringList(v) => query.bind(v),
            SqlParam::NumberList(v) => query.bind(v),
        };
    }
    // One SELECT snapshot, streamed to enforce memory bounds before accumulating
    // the next entity. The receiver gets the complete matching set or no job.
    let mut stream = query.fetch(&mut **tx);
    let mut data = Vec::new();
    let mut bytes = serde_json::to_vec(&Notification::new(&sub.id, Vec::new()))
        .map_err(err)?
        .len()
        + 16; // bounded envelope including subscription ID
    while let Some(row) = stream.try_next().await.map_err(err)? {
        if data.len() >= config.scheduler_max_entities {
            return Err("Periodic snapshot exceeds scheduler_max_entities".into());
        }
        let entity = Entity {
            id: row.try_get("id").map_err(err)?,
            type_: row.try_get("type").map_err(err)?,
            types: row.try_get("types").map_err(err)?,
            attributes: serde_json::from_value(row.try_get("attrs").map_err(err)?).map_err(err)?,
            scope: row.try_get("scope").map_err(err)?,
            created_at: Some(row.try_get("created_at").map_err(err)?),
            modified_at: Some(row.try_get("modified_at").map_err(err)?),
            context: None,
        };
        let value = NotificationDispatcher::build_notification_data(sub, &entity);
        bytes += serde_json::to_vec(&value).map_err(err)?.len() + 1;
        if bytes > config.scheduler_max_payload_bytes {
            return Err("Periodic snapshot exceeds scheduler_max_payload_bytes".into());
        }
        data.push(value);
    }
    Ok(data)
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
