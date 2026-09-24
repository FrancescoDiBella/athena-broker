use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "idPattern")]
    pub id_pattern: Option<String>,
    #[serde(rename = "type")]
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "receiverInfo")]
    pub receiver_info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "notifierInfo")]
    pub notifier_info: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>, // "normalized" or "keyValues"
    pub endpoint: Endpoint,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastNotification")]
    pub last_notification: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastFailure")]
    pub last_failure: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSuccess")]
    pub last_success: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "timesSent")]
    pub times_sent: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Expired,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subscription {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "subscriptionName")]
    pub subscription_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityInfo>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "watchedAttributes")]
    pub watched_attributes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "geoQ")]
    pub geo_q: Option<serde_json::Value>,
    pub notification: NotificationParams,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttling: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "timeInterval")]
    pub time_interval: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "isActive")]
    pub is_active: Option<bool>,
    #[serde(default)]
    pub status: SubscriptionStatus,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "@context")]
    pub context: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Notification {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String, // "Notification"
    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,
    #[serde(rename = "notifiedAt")]
    pub notified_at: DateTime<Utc>,
    pub data: Vec<serde_json::Value>,
}

impl Notification {
    pub fn new(subscription_id: impl Into<String>, data: Vec<serde_json::Value>) -> Self {
        Self {
            id: format!("urn:ngsi-ld:Notification:{}", uuid::Uuid::new_v4()),
            r#type: "Notification".to_string(),
            subscription_id: subscription_id.into(),
            notified_at: Utc::now(),
            data,
        }
    }
}

impl Subscription {
    /// Structural checks shared by API and transactionally merged storage updates.
    pub fn validate(&self) -> Result<(), String> {
        if self.r#type != "Subscription" || !crate::attributes::valid_uri(&self.id) {
            return Err("Invalid subscription identity".into());
        }
        if self.entities.is_empty() && self.watched_attributes.is_none() {
            return Err("entities or watchedAttributes must be provided".into());
        }
        if self.entities.len() > 1000 {
            return Err("Too many entity selectors".into());
        }
        for target in &self.entities {
            if target.r#type.is_empty() {
                return Err("Selector type cannot be empty".into());
            }
            if target.id.is_some() && target.id_pattern.is_some() {
                return Err("id and idPattern are mutually exclusive".into());
            }
            if target
                .id
                .as_ref()
                .is_some_and(|id| !crate::attributes::valid_uri(id))
            {
                return Err("Invalid selector id".into());
            }
            if let Some(pattern) = &target.id_pattern {
                if pattern.len() > 4096 {
                    return Err("idPattern exceeds 4096 bytes".into());
                }
                regex::RegexBuilder::new(pattern)
                    .size_limit(1024 * 1024)
                    .build()
                    .map_err(|_| "Invalid or excessively complex idPattern")?;
            }
        }
        for attrs in [&self.watched_attributes, &self.notification.attributes]
            .into_iter()
            .flatten()
        {
            if attrs.is_empty() || attrs.len() > 1000 || attrs.iter().any(|v| v.is_empty()) {
                return Err("Attribute lists require 1..1000 non-empty entries".into());
            }
        }
        if self
            .throttling
            .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 31536000.0)
        {
            return Err("throttling must be positive and at most one year".into());
        }
        if let Some(interval) = self.time_interval {
            if !interval.is_finite() || interval <= 0.0 || interval > 31536000.0 {
                return Err("timeInterval must be positive and at most one year".into());
            }
            if self.watched_attributes.is_some() || self.throttling.is_some() {
                return Err(
                    "timeInterval cannot be combined with watchedAttributes or throttling".into(),
                );
            }
        }
        if !matches!(
            self.notification.format.as_deref(),
            None | Some("normalized" | "keyValues" | "simplified")
        ) {
            return Err("Unsupported notification format".into());
        }
        if !matches!(
            self.notification.endpoint.accept.as_deref(),
            None | Some("application/json" | "application/ld+json")
        ) {
            return Err("Unsupported notification media type".into());
        }
        let endpoint =
            url::Url::parse(&self.notification.endpoint.uri).map_err(|_| "Invalid endpoint URI")?;
        if matches!(endpoint.scheme(), "mqtt" | "mqtts") {
            self.notification.endpoint.mqtt_options()?;
            return Ok(());
        }
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("Endpoint must be HTTP(S), without URL credentials or fragment".into());
        }
        if self.notification.endpoint.notifier_info.is_some() {
            return Err("notifierInfo is only supported for MQTT endpoints".into());
        }
        Ok(())
    }
}

/// Ignore server-generated fields, reject unsupported options instead of accepting
/// a subscription with silently different behavior. JSON null is not NGSI-LD null.
pub fn sanitize_subscription_input(
    value: &mut serde_json::Value,
    create: bool,
) -> Result<(), String> {
    let object = value
        .as_object_mut()
        .ok_or("Subscription must be an object")?;
    for key in ["status", "createdAt", "modifiedAt"] {
        object.remove(key);
    }
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "id" | "type"
                | "subscriptionName"
                | "description"
                | "entities"
                | "watchedAttributes"
                | "q"
                | "geoQ"
                | "notification"
                | "throttling"
                | "timeInterval"
                | "expiresAt"
                | "isActive"
                | "@context"
        ) {
            return Err(format!("Unsupported subscription field: {key}"));
        }
        if !create && matches!(key.as_str(), "id" | "type") {
            return Err("Subscription identity is immutable".into());
        }
    }
    if let Some(v) = object.get("isActive") {
        if !v.is_boolean() {
            return Err("isActive must be boolean".into());
        }
    }
    if let Some(v) = object.get("entities") {
        if !v.is_array() || v.as_array().is_some_and(Vec::is_empty) {
            return Err("entities must be a non-empty array".into());
        }
    }
    for (key, v) in object.iter() {
        if v.is_null() {
            return Err(format!(
                "JSON null is not permitted for {key}; use urn:ngsi-ld:null for optional fields"
            ));
        }
    }
    if let Some(v) = object.get("expiresAt").filter(|v| *v != "urn:ngsi-ld:null") {
        let timestamp = v
            .as_str()
            .ok_or("expiresAt must be a DateTime")?
            .parse::<DateTime<Utc>>()
            .map_err(|_| "Invalid expiresAt")?;
        if timestamp <= Utc::now() {
            return Err("expiresAt must be in the future".into());
        }
    }
    if let Some(notification) = object.get_mut("notification") {
        let n = notification
            .as_object_mut()
            .ok_or("notification must be an object")?;
        for key in [
            "lastNotification",
            "lastFailure",
            "lastSuccess",
            "timesSent",
            "timesFailed",
            "status",
        ] {
            n.remove(key);
        }
        if n.keys()
            .any(|key| !matches!(key.as_str(), "attributes" | "format" | "endpoint"))
        {
            return Err("Unsupported notification option".into());
        }
        let endpoint = n
            .get("endpoint")
            .and_then(serde_json::Value::as_object)
            .ok_or("notification replacement requires endpoint")?;
        if endpoint.keys().any(|key| {
            !matches!(
                key.as_str(),
                "uri" | "accept" | "receiverInfo" | "notifierInfo"
            )
        }) {
            return Err("Unsupported endpoint option".into());
        }
    }
    Ok(())
}
