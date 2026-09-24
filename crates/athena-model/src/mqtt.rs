//! Validated MQTT endpoint settings. Intentionally no Debug: URI credentials are secrets.
use crate::Endpoint;
use percent_encoding::percent_decode_str;

pub struct MqttEndpoint {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub topic: String,
    pub username: String,
    pub password: Option<String>,
    pub qos: u8,
    pub v5: bool,
}

impl Endpoint {
    pub fn mqtt_options(&self) -> Result<MqttEndpoint, String> {
        let uri = url::Url::parse(&self.uri).map_err(|_| "Invalid MQTT endpoint")?;
        if !matches!(uri.scheme(), "mqtt" | "mqtts")
            || uri.host().is_none()
            || uri.fragment().is_some()
            || uri.query().is_some()
        {
            return Err(
                "MQTT endpoint requires mqtt[s]://host/topic without query or fragment".into(),
            );
        }
        let decode = |s: &str| -> Result<String, String> {
            let value = percent_decode_str(s)
                .decode_utf8()
                .map_err(|_| "Invalid MQTT UTF-8")?
                .into_owned();
            if value.len() > 65535
                || value.chars().any(|c| {
                    c.is_control()
                        || matches!(c as u32, 0xFDD0..=0xFDEF)
                        || c as u32 & 0xffff >= 0xfffe
                })
            {
                return Err("Invalid MQTT string".into());
            }
            Ok(value)
        };
        let topic = decode(uri.path().strip_prefix('/').unwrap_or(uri.path()))?;
        if topic.is_empty() || topic.contains(['#', '+']) {
            return Err(
                "MQTT notification topic must be non-empty and contain no wildcards".into(),
            );
        }
        let mut qos = 0;
        let mut v5 = true;
        if let Some(info) = &self.notifier_info {
            let entries = info
                .as_array()
                .ok_or("notifierInfo must be a KeyValuePair array")?;
            let mut seen = std::collections::HashSet::new();
            for entry in entries {
                let key = entry
                    .get("key")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("notifierInfo requires key")?;
                let value = entry
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("notifierInfo requires string value")?;
                if !seen.insert(key) {
                    return Err("Duplicate notifierInfo key".into());
                }
                match (key, value) {
                    ("MQTT-QoS", "0" | "1" | "2") => qos = value.parse().unwrap(),
                    ("MQTT-Version", "mqtt3.1.1") => v5 = false,
                    ("MQTT-Version", "mqtt5.0") => v5 = true,
                    _ => return Err("Unsupported MQTT notifierInfo key or value".into()),
                }
            }
        }
        let host = match uri.host().unwrap() {
            url::Host::Domain(value) => value.to_string(),
            url::Host::Ipv4(value) => value.to_string(),
            url::Host::Ipv6(value) => value.to_string(),
        };
        let port = uri
            .port()
            .unwrap_or(if uri.scheme() == "mqtts" { 8883 } else { 1883 });
        if port == 0 {
            return Err("MQTT port cannot be zero".into());
        }
        Ok(MqttEndpoint {
            host,
            port,
            tls: uri.scheme() == "mqtts",
            topic,
            username: decode(uri.username())?,
            password: uri.password().map(decode).transpose()?,
            qos,
            v5,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn endpoint(uri: &str) -> Endpoint {
        serde_json::from_value(json!({"uri":uri})).unwrap()
    }
    #[test]
    fn mqtt_options_are_strict_and_decode_uri() {
        let settings = endpoint("mqtts://user:p%40ss@example.org/floor%20one/temp")
            .mqtt_options()
            .unwrap();
        assert_eq!(settings.password.as_deref(), Some("p@ss"));
        assert_eq!(settings.topic, "floor one/temp");
        assert_eq!(settings.port, 8883);
        assert!(settings.v5);
        for uri in [
            "mqtt://example.org/",
            "mqtt://example.org/a/%23",
            "mqtt://example.org/a?x",
            "mqtt://example.org/a%00b",
        ] {
            assert!(endpoint(uri).mqtt_options().is_err());
        }
        let mut endpoint = endpoint("mqtt://example.org/events");
        endpoint.notifier_info = Some(json!([{"key":"MQTT-QoS","value":"3"}]));
        assert!(endpoint.mqtt_options().is_err());
    }
}
