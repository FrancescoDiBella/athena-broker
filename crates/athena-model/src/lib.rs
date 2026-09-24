pub mod attributes;
pub mod batch;
pub mod csource;
pub mod entity;
pub mod error;
pub mod geoproperty;
pub mod headers;
pub mod property;
pub mod relationship;
pub mod subscription;
pub mod temporal;

pub use batch::{BatchEntityError, BatchOperationResult};
pub use csource::{CsourceRegistration, RegistrationInfo, RegistrationStatus, TimeInterval};
pub use entity::Entity;
pub use error::{ModelError, ProblemDetails};
pub use geoproperty::{GeoProperty, Geometry};
pub use headers::{
    LinkHeader, APPLICATION_JSON, APPLICATION_LD_JSON, ETSI_CORE_CONTEXT_URL, JSON_LD_CONTEXT_REL,
};
pub use property::Property;
pub use relationship::Relationship;
pub use subscription::{
    Endpoint, EntityInfo, Notification, NotificationParams, Subscription, SubscriptionStatus,
};
pub use temporal::{AggrMethod, TemporalQuery, TimeProperty, TimeRel};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_entity_serialization_normalized() {
        let mut entity = Entity::new("urn:ngsi-ld:Vehicle:A100", "Vehicle");
        entity
            .set_property("speed", Property::new(85.5).with_unit_code("KMH"))
            .unwrap();
        entity
            .set_relationship("isOwnedBy", Relationship::new("urn:ngsi-ld:Person:Bob"))
            .unwrap();
        entity
            .set_geoproperty("location", GeoProperty::new_point(13.4050, 52.5200))
            .unwrap();

        let normalized = entity.to_normalized(false);
        assert_eq!(normalized["id"], "urn:ngsi-ld:Vehicle:A100");
        assert_eq!(normalized["type"], "Vehicle");
        assert_eq!(normalized["speed"]["type"], "Property");
        assert_eq!(normalized["speed"]["value"], 85.5);
        assert_eq!(normalized["speed"]["unitCode"], "KMH");
        assert_eq!(normalized["isOwnedBy"]["type"], "Relationship");
        assert_eq!(normalized["isOwnedBy"]["object"], "urn:ngsi-ld:Person:Bob");
        assert_eq!(normalized["location"]["type"], "GeoProperty");
        assert_eq!(normalized["location"]["value"]["type"], "Point");
    }

    #[test]
    fn test_entity_serialization_keyvalues() {
        let mut entity = Entity::new("urn:ngsi-ld:Vehicle:A100", "Vehicle");
        entity
            .set_property("speed", Property::new(85.5).with_unit_code("KMH"))
            .unwrap();
        entity
            .set_relationship("isOwnedBy", Relationship::new("urn:ngsi-ld:Person:Bob"))
            .unwrap();
        entity
            .set_geoproperty("location", GeoProperty::new_point(13.4050, 52.5200))
            .unwrap();

        let kv = entity.to_key_values();
        assert_eq!(kv["id"], "urn:ngsi-ld:Vehicle:A100");
        assert_eq!(kv["type"], "Vehicle");
        assert_eq!(kv["speed"], 85.5);
        assert_eq!(kv["isOwnedBy"], "urn:ngsi-ld:Person:Bob");
        assert_eq!(kv["location"]["type"], "Point");
        assert_eq!(kv["location"]["coordinates"][0], 13.4050);
        assert_eq!(kv["location"]["coordinates"][1], 52.5200);
    }

    #[test]
    fn test_entity_from_json() {
        let json_data = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "temperature": {
                "type": "Property",
                "value": 21.5
            },
            "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        });

        let entity = Entity::from_json(json_data).unwrap();
        assert_eq!(entity.id, "urn:ngsi-ld:Building:001");
        assert_eq!(entity.type_, "Building");
        let prop = entity.get_property("temperature").unwrap();
        assert_eq!(prop.value, 21.5);
    }

    #[test]
    fn test_link_header_parsing() {
        let raw = r#"<https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
        let parsed = LinkHeader::parse(raw).unwrap();
        assert_eq!(
            parsed.uri,
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        );
        assert_eq!(parsed.rel, JSON_LD_CONTEXT_REL);
        assert_eq!(parsed.mime_type.as_deref(), Some("application/ld+json"));
    }
}
pub mod mqtt;
