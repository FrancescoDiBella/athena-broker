use serde_json::{Map, Value};

use athena_model::{Entity, Subscription};

pub struct NotificationDispatcher;

impl NotificationDispatcher {
    pub async fn send(
        sub: &Subscription,
        notification: &Value,
        client: &athena_http::OutboundClient,
    ) -> Result<(), String> {
        Self::send_with_mqtt(
            sub,
            notification,
            client,
            &crate::mqtt::MqttClient::from_ca_file(None)?,
        )
        .await
    }
    pub async fn send_with_mqtt(
        sub: &Subscription,
        notification: &Value,
        client: &athena_http::OutboundClient,
        mqtt: &crate::mqtt::MqttClient,
    ) -> Result<(), String> {
        if url::Url::parse(&sub.notification.endpoint.uri)
            .is_ok_and(|uri| matches!(uri.scheme(), "mqtt" | "mqtts"))
        {
            return mqtt.send(sub, notification, client).await;
        }
        let mut body = notification.clone();
        if sub.notification.endpoint.accept.as_deref() == Some("application/ld+json") {
            body["@context"] = serde_json::json!(athena_model::ETSI_CORE_CONTEXT_URL);
        }
        let mut req = client
            .request(reqwest::Method::POST, &sub.notification.endpoint.uri)?
            .json(&body);
        let content_type = sub
            .notification
            .endpoint
            .accept
            .as_deref()
            .unwrap_or("application/json");
        req = req.header("Content-Type", content_type);
        if content_type == "application/json" {
            req = req.header("Link", "<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        }
        for (key, value) in receiver_headers(sub.notification.endpoint.receiver_info.as_ref())? {
            req = req.header(key, value);
        }
        let response = req.send().await.map_err(|e| e.to_string())?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "Notification endpoint returned {}",
                response.status()
            ))
        }
    }

    pub fn build_notification_data(sub: &Subscription, entity: &Entity) -> Value {
        let is_key_values = sub
            .notification
            .format
            .as_deref()
            .map(|f| matches!(f, "keyValues" | "simplified"))
            .unwrap_or(false);

        let base_val = if is_key_values {
            entity.to_key_values()
        } else {
            entity.to_normalized(false)
        };

        // Filter attributes if notification attributes list is specified
        if let Some(filter_attrs) = &sub.notification.attributes {
            if !filter_attrs.is_empty() {
                if let Value::Object(map) = base_val {
                    let mut filtered = Map::new();
                    // Always preserve id and type
                    if let Some(id) = map.get("id") {
                        filtered.insert("id".to_string(), id.clone());
                    }
                    if let Some(t) = map.get("type") {
                        filtered.insert("type".to_string(), t.clone());
                    }
                    if let Some(ctx) = map.get("@context") {
                        filtered.insert("@context".to_string(), ctx.clone());
                    }

                    for attr in filter_attrs {
                        if let Some(v) = map.get(attr).or_else(|| {
                            attr.strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
                                .and_then(|key| map.get(key))
                        }) {
                            filtered.insert(attr.clone(), v.clone());
                        }
                    }
                    return Value::Object(filtered);
                }
            }
        }

        base_val
    }
}

pub fn validate_endpoint(
    policy: athena_http::OutboundPolicy,
    endpoint: &athena_model::Endpoint,
) -> Result<(), String> {
    let uri = url::Url::parse(&endpoint.uri).map_err(|_| "Invalid endpoint URL")?;
    if matches!(uri.scheme(), "mqtt" | "mqtts") {
        let settings = endpoint.mqtt_options()?;
        policy.validate_host(&settings.host)
    } else {
        if endpoint.notifier_info.is_some() {
            return Err("notifierInfo is only supported for MQTT".into());
        }
        policy.validate(&uri)
    }
}

/// NGSI-LD receiverInfo is a KeyValuePair array. Accept the legacy headers object
/// as well, but never let it override framing or the notification representation.
pub fn receiver_headers(
    info: Option<&Value>,
) -> Result<Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>, String> {
    let Some(info) = info else {
        return Ok(Vec::new());
    };
    let pairs: Vec<(&str, &str)> = if let Some(entries) = info.as_array() {
        entries
            .iter()
            .map(|entry| {
                Ok((
                    entry
                        .get("key")
                        .and_then(Value::as_str)
                        .ok_or("receiverInfo requires key")?,
                    entry
                        .get("value")
                        .and_then(Value::as_str)
                        .ok_or("receiverInfo requires string value")?,
                ))
            })
            .collect::<Result<_, String>>()?
    } else if let Some(headers) = info.get("headers").and_then(Value::as_object) {
        headers
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.as_str(),
                    value.as_str().ok_or("Header value must be a string")?,
                ))
            })
            .collect::<Result<_, String>>()?
    } else {
        return Err("receiverInfo must be a KeyValuePair array".into());
    };
    if pairs.len() > 64 {
        return Err("Too many receiverInfo headers".into());
    }
    pairs
        .into_iter()
        .map(|(key, value)| {
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "content-type"
                    | "link"
                    | "trailer"
                    | "te"
                    | "upgrade"
                    | "expect"
                    | "proxy-authorization"
                    | "proxy-authenticate"
                    | "keep-alive"
            ) {
                return Err("receiverInfo cannot override protocol headers".into());
            }
            if value.len() > 8192 {
                return Err("receiverInfo header value too long".into());
            }
            Ok((
                reqwest::header::HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| "Invalid receiverInfo header name")?,
                reqwest::header::HeaderValue::from_str(value)
                    .map_err(|_| "Invalid receiverInfo header value")?,
            ))
        })
        .collect()
}
