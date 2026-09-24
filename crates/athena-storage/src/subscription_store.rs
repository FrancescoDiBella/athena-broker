use crate::repository::{StorageError, SubscriptionRepository};
use async_trait::async_trait;
use athena_model::{ProblemDetails, Subscription, SubscriptionStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{postgres::PgRow, PgPool, Row};

pub struct PgSubscriptionStore {
    pool: PgPool,
}
impl PgSubscriptionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
fn bad(message: impl Into<String>) -> StorageError {
    ProblemDetails::bad_request_data(message).into()
}
fn validate(sub: &Subscription) -> Result<(), StorageError> {
    sub.validate().map_err(bad)?;
    if let Some(q) = &sub.q {
        athena_query::Parser::parse_str(q).map_err(|e| bad(e.to_string()))?;
    }
    if let Some(geo) = &sub.geo_q {
        athena_query::GeoQueryParser::parse(
            geo.get("georel").and_then(Value::as_str),
            geo.get("geometry").and_then(Value::as_str),
            geo.get("coordinates").map(Value::to_string).as_deref(),
            geo.get("geoproperty").and_then(Value::as_str),
        )
        .map_err(|e| bad(e.to_string()))?
        .ok_or_else(|| bad("Incomplete geoQ"))?;
    }
    Ok(())
}
pub fn decode(row: PgRow) -> Result<Subscription, StorageError> {
    let expires: Option<DateTime<Utc>> = row.try_get("expires_at")?;
    let status: String = row.try_get("status")?;
    let status = if expires.is_some_and(|t| t <= Utc::now()) {
        SubscriptionStatus::Expired
    } else {
        match status.as_str() {
            "paused" => SubscriptionStatus::Paused,
            "expired" => SubscriptionStatus::Expired,
            _ => SubscriptionStatus::Active,
        }
    };
    Ok(Subscription {
        id: row.try_get("id")?,
        r#type: "Subscription".into(),
        subscription_name: row.try_get("subscription_name")?,
        description: row.try_get("description")?,
        entities: serde_json::from_value(row.try_get("entities")?)?,
        watched_attributes: row.try_get("watched_attributes")?,
        q: row.try_get("q")?,
        geo_q: row.try_get("geo_q")?,
        notification: serde_json::from_value(row.try_get("notification")?)?,
        throttling: row.try_get("throttling")?,
        time_interval: row.try_get("time_interval")?,
        expires_at: expires,
        is_active: Some(status == SubscriptionStatus::Active),
        status,
        created_at: Some(row.try_get("created_at")?),
        modified_at: Some(row.try_get("modified_at")?),
        context: row.try_get("context")?,
    })
}
fn status(sub: &Subscription) -> &'static str {
    match sub.status {
        SubscriptionStatus::Active => "active",
        SubscriptionStatus::Paused => "paused",
        SubscriptionStatus::Expired => "expired",
    }
}

#[async_trait]
impl SubscriptionRepository for PgSubscriptionStore {
    async fn create_subscription(&self, sub: &Subscription) -> Result<(), StorageError> {
        validate(sub)?;
        let mut sub = sub.clone();
        sub.status = if sub.is_active == Some(false) {
            SubscriptionStatus::Paused
        } else {
            SubscriptionStatus::Active
        };
        if sub.expires_at.is_some_and(|t| t <= Utc::now()) {
            return Err(bad("expiresAt must be in the future"));
        }
        let mut notification = serde_json::to_value(&sub.notification)?;
        for key in [
            "lastNotification",
            "lastSuccess",
            "lastFailure",
            "timesSent",
        ] {
            notification.as_object_mut().unwrap().remove(key);
        }
        sqlx::query("INSERT INTO subscriptions (id,subscription_name,description,entities,watched_attributes,q,geo_q,notification,throttling,time_interval,expires_at,status,context) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
            .bind(&sub.id).bind(&sub.subscription_name).bind(&sub.description).bind(serde_json::to_value(&sub.entities)?)
            .bind(&sub.watched_attributes).bind(&sub.q).bind(&sub.geo_q).bind(notification).bind(sub.throttling)
            .bind(sub.time_interval).bind(sub.expires_at).bind(status(&sub)).bind(&sub.context).execute(&self.pool).await?;
        Ok(())
    }
    async fn get_subscription_by_id(&self, id: &str) -> Result<Option<Subscription>, StorageError> {
        sqlx::query("SELECT * FROM subscriptions WHERE id=$1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(decode)
            .transpose()
    }
    async fn list_subscriptions(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Subscription>, StorageError> {
        sqlx::query("SELECT * FROM subscriptions ORDER BY created_at DESC,id LIMIT $1 OFFSET $2")
            .bind(limit.unwrap_or(20).clamp(0, 1000))
            .bind(offset.unwrap_or(0).max(0))
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(decode)
            .collect()
    }
    async fn update_subscription(&self, id: &str, patch: &Value) -> Result<(), StorageError> {
        let mut patch = patch.clone();
        athena_model::subscription::sanitize_subscription_input(&mut patch, false).map_err(bad)?;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT * FROM subscriptions WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StorageError::SubscriptionNotFound(id.into()))?;
        let old = decode(row)?;
        let mut value = serde_json::to_value(&old)?;
        for (key, v) in patch
            .as_object()
            .ok_or_else(|| bad("Expected subscription object"))?
        {
            if v == "urn:ngsi-ld:null" {
                if !matches!(
                    key.as_str(),
                    "description"
                        | "subscriptionName"
                        | "q"
                        | "geoQ"
                        | "throttling"
                        | "expiresAt"
                        | "watchedAttributes"
                        | "timeInterval"
                ) {
                    return Err(bad("Cannot remove this subscription field"));
                }
                value.as_object_mut().unwrap().remove(key);
            } else {
                value[key] = v.clone();
            }
        }
        let mut sub: Subscription =
            serde_json::from_value(value).map_err(|e| bad(e.to_string()))?;
        // Replacement at the first level, as required by 5.5.8, with immutable counters.
        sub.notification.last_notification = old.notification.last_notification;
        sub.notification.last_success = old.notification.last_success;
        sub.notification.last_failure = old.notification.last_failure;
        sub.notification.times_sent = old.notification.times_sent;
        let renewed = patch
            .get("expiresAt")
            .is_some_and(|v| v != "urn:ngsi-ld:null");
        sub.status = if let Some(active) = patch.get("isActive").and_then(Value::as_bool) {
            if old.status == SubscriptionStatus::Expired && !renewed {
                SubscriptionStatus::Expired
            } else if active {
                SubscriptionStatus::Active
            } else {
                SubscriptionStatus::Paused
            }
        } else if renewed && old.status == SubscriptionStatus::Expired {
            SubscriptionStatus::Active
        } else {
            old.status
        };
        validate(&sub)?;
        sqlx::query("UPDATE subscriptions SET subscription_name=$2,description=$3,entities=$4,watched_attributes=$5,q=$6,geo_q=$7,notification=$8,throttling=$9,time_interval=$10,expires_at=$11,status=$12,context=$13,modified_at=clock_timestamp() WHERE id=$1")
            .bind(id).bind(&sub.subscription_name).bind(&sub.description).bind(serde_json::to_value(&sub.entities)?)
            .bind(&sub.watched_attributes).bind(&sub.q).bind(&sub.geo_q).bind(serde_json::to_value(&sub.notification)?)
            .bind(sub.throttling).bind(sub.time_interval).bind(sub.expires_at).bind(status(&sub)).bind(&sub.context)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn delete_subscription(&self, id: &str) -> Result<(), StorageError> {
        if sqlx::query("DELETE FROM subscriptions WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            == 0
        {
            return Err(StorageError::SubscriptionNotFound(id.into()));
        }
        Ok(())
    }
    async fn get_active_subscriptions(&self) -> Result<Vec<Subscription>, StorageError> {
        sqlx::query("SELECT * FROM subscriptions WHERE status='active' AND (expires_at IS NULL OR expires_at>clock_timestamp())")
            .fetch_all(&self.pool).await?.into_iter().map(decode).collect()
    }
    async fn record_notification_success(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("UPDATE subscriptions SET last_notification=clock_timestamp(),notification=notification||jsonb_build_object('lastNotification',clock_timestamp(),'lastSuccess',clock_timestamp(),'timesSent',COALESCE((notification->>'timesSent')::bigint,0)+1) WHERE id=$1")
            .bind(id).execute(&self.pool).await?;
        Ok(())
    }
    async fn record_notification_failure(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("UPDATE subscriptions SET notification=notification||jsonb_build_object('lastFailure',clock_timestamp()) WHERE id=$1").bind(id).execute(&self.pool).await?;
        Ok(())
    }
}
