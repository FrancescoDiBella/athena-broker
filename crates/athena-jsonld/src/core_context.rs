use std::collections::HashMap;

pub const ETSI_CORE_CONTEXT_V1_8: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
pub const ETSI_CORE_CONTEXT_V1_7: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld";
pub const ETSI_CORE_CONTEXT_V1_6: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld";
pub const ETSI_CORE_CONTEXT_V1_5: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.5.jsonld";
pub const ETSI_CORE_CONTEXT_V1_4: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.4.jsonld";
pub const ETSI_CORE_CONTEXT_V1_3: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.3.jsonld";
pub const ETSI_CORE_CONTEXT_LATEST: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld";

pub fn is_etsi_core_context(uri: &str) -> bool {
    let clean = uri.trim();
    clean == ETSI_CORE_CONTEXT_V1_8
        || clean == ETSI_CORE_CONTEXT_V1_7
        || clean == ETSI_CORE_CONTEXT_V1_6
        || clean == ETSI_CORE_CONTEXT_V1_5
        || clean == ETSI_CORE_CONTEXT_V1_4
        || clean == ETSI_CORE_CONTEXT_V1_3
        || clean == ETSI_CORE_CONTEXT_LATEST
        || clean == "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
}

pub fn get_etsi_core_mappings() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("id".to_string(), "@id".to_string());
    map.insert("type".to_string(), "@type".to_string());
    map.insert(
        "Property".to_string(),
        "https://uri.etsi.org/ngsi-ld/Property".to_string(),
    );
    map.insert(
        "Relationship".to_string(),
        "https://uri.etsi.org/ngsi-ld/Relationship".to_string(),
    );
    map.insert(
        "GeoProperty".to_string(),
        "https://uri.etsi.org/ngsi-ld/GeoProperty".to_string(),
    );
    map.insert(
        "TemporalProperty".to_string(),
        "https://uri.etsi.org/ngsi-ld/TemporalProperty".to_string(),
    );
    map.insert(
        "LanguageProperty".to_string(),
        "https://uri.etsi.org/ngsi-ld/LanguageProperty".to_string(),
    );
    map.insert(
        "value".to_string(),
        "https://uri.etsi.org/ngsi-ld/hasValue".to_string(),
    );
    map.insert(
        "object".to_string(),
        "https://uri.etsi.org/ngsi-ld/hasObject".to_string(),
    );
    map.insert(
        "objects".to_string(),
        "https://uri.etsi.org/ngsi-ld/hasObjects".to_string(),
    );
    map.insert(
        "languageMap".to_string(),
        "https://uri.etsi.org/ngsi-ld/hasLanguageMap".to_string(),
    );
    map.insert(
        "observedAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/observedAt".to_string(),
    );
    map.insert(
        "createdAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/createdAt".to_string(),
    );
    map.insert(
        "modifiedAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/modifiedAt".to_string(),
    );
    map.insert(
        "deletedAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/deletedAt".to_string(),
    );
    map.insert(
        "datasetId".to_string(),
        "https://uri.etsi.org/ngsi-ld/datasetId".to_string(),
    );
    map.insert(
        "instanceId".to_string(),
        "https://uri.etsi.org/ngsi-ld/instanceId".to_string(),
    );
    map.insert(
        "unitCode".to_string(),
        "https://uri.etsi.org/ngsi-ld/unitCode".to_string(),
    );
    map.insert(
        "location".to_string(),
        "https://uri.etsi.org/ngsi-ld/location".to_string(),
    );
    map.insert(
        "observationSpace".to_string(),
        "https://uri.etsi.org/ngsi-ld/observationSpace".to_string(),
    );
    map.insert(
        "operationSpace".to_string(),
        "https://uri.etsi.org/ngsi-ld/operationSpace".to_string(),
    );
    map.insert(
        "Subscription".to_string(),
        "https://uri.etsi.org/ngsi-ld/Subscription".to_string(),
    );
    map.insert(
        "Notification".to_string(),
        "https://uri.etsi.org/ngsi-ld/Notification".to_string(),
    );
    map.insert(
        "entities".to_string(),
        "https://uri.etsi.org/ngsi-ld/entities".to_string(),
    );
    map.insert(
        "notification".to_string(),
        "https://uri.etsi.org/ngsi-ld/notification".to_string(),
    );
    map.insert(
        "endpoint".to_string(),
        "https://uri.etsi.org/ngsi-ld/endpoint".to_string(),
    );
    map.insert(
        "uri".to_string(),
        "https://uri.etsi.org/ngsi-ld/uri".to_string(),
    );
    map.insert(
        "status".to_string(),
        "https://uri.etsi.org/ngsi-ld/status".to_string(),
    );
    map.insert(
        "q".to_string(),
        "https://uri.etsi.org/ngsi-ld/q".to_string(),
    );
    map.insert(
        "geoQ".to_string(),
        "https://uri.etsi.org/ngsi-ld/geoQ".to_string(),
    );
    map.insert(
        "georel".to_string(),
        "https://uri.etsi.org/ngsi-ld/georel".to_string(),
    );
    map.insert(
        "geometry".to_string(),
        "https://uri.etsi.org/ngsi-ld/geometry".to_string(),
    );
    map.insert(
        "coordinates".to_string(),
        "https://uri.etsi.org/ngsi-ld/coordinates".to_string(),
    );
    map.insert(
        "geoproperty".to_string(),
        "https://uri.etsi.org/ngsi-ld/geoproperty".to_string(),
    );
    map.insert(
        "timerel".to_string(),
        "https://uri.etsi.org/ngsi-ld/timerel".to_string(),
    );
    map.insert(
        "timeAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/timeAt".to_string(),
    );
    map.insert(
        "endTimeAt".to_string(),
        "https://uri.etsi.org/ngsi-ld/endTimeAt".to_string(),
    );
    map.insert(
        "timeproperty".to_string(),
        "https://uri.etsi.org/ngsi-ld/timeproperty".to_string(),
    );
    map.insert(
        "aggrMethod".to_string(),
        "https://uri.etsi.org/ngsi-ld/aggrMethod".to_string(),
    );
    map.insert(
        "aggrPeriodDuration".to_string(),
        "https://uri.etsi.org/ngsi-ld/aggrPeriodDuration".to_string(),
    );
    map.insert(
        "Point".to_string(),
        "https://uri.etsi.org/ngsi-ld/Point".to_string(),
    );
    map.insert(
        "MultiPoint".to_string(),
        "https://uri.etsi.org/ngsi-ld/MultiPoint".to_string(),
    );
    map.insert(
        "LineString".to_string(),
        "https://uri.etsi.org/ngsi-ld/LineString".to_string(),
    );
    map.insert(
        "MultiLineString".to_string(),
        "https://uri.etsi.org/ngsi-ld/MultiLineString".to_string(),
    );
    map.insert(
        "Polygon".to_string(),
        "https://uri.etsi.org/ngsi-ld/Polygon".to_string(),
    );
    map.insert(
        "MultiPolygon".to_string(),
        "https://uri.etsi.org/ngsi-ld/MultiPolygon".to_string(),
    );
    map
}
