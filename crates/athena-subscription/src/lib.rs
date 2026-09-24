pub mod config;
pub use config::EngineConfig;
pub mod dispatcher;
pub mod engine;
pub mod matcher;
pub mod mqtt;
pub mod scheduler;

pub use dispatcher::NotificationDispatcher;
pub use engine::SubscriptionEngine;
pub use matcher::SubscriptionMatcher;

#[cfg(test)]
mod tests {
    use super::*;
    use athena_model::{
        Endpoint, EntityInfo, NotificationParams, Property, Subscription, SubscriptionStatus,
    };

    #[test]
    fn test_subscription_matching() {
        let mut entity = athena_model::Entity::new("urn:ngsi-ld:Vehicle:A100", "Vehicle");
        entity.set_property("speed", Property::new(95.0)).unwrap();

        let sub = Subscription {
            id: "urn:ngsi-ld:Subscription:001".to_string(),
            r#type: "Subscription".to_string(),
            subscription_name: Some("Speed Alert".to_string()),
            description: None,
            entities: vec![EntityInfo {
                id: None,
                id_pattern: Some(".*Vehicle.*".to_string()),
                r#type: "Vehicle".to_string(),
            }],
            watched_attributes: Some(vec!["speed".to_string()]),
            q: Some("speed>80".to_string()),
            geo_q: None,
            notification: NotificationParams {
                attributes: None,
                format: Some("keyValues".to_string()),
                endpoint: Endpoint {
                    uri: "http://example.org/notify".to_string(),
                    accept: Some("application/json".to_string()),
                    receiver_info: None,
                    notifier_info: None,
                },
                last_notification: None,
                last_failure: None,
                last_success: None,
                times_sent: None,
            },
            throttling: None,
            time_interval: None,
            expires_at: None,
            is_active: Some(true),
            status: SubscriptionStatus::Active,
            created_at: None,
            modified_at: None,
            context: None,
        };

        let mutated = vec!["speed".to_string()];
        assert!(SubscriptionMatcher::matches(&sub, &entity, &mutated));

        // When non-watched attribute is mutated
        let mutated_other = vec!["temperature".to_string()];
        assert!(!SubscriptionMatcher::matches(&sub, &entity, &mutated_other));
    }
}
