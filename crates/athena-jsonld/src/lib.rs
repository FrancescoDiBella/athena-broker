pub mod core_context;
pub mod expander;
pub mod resolver;

pub use core_context::{get_etsi_core_mappings, is_etsi_core_context};
pub use expander::{compact_entity, expand_entity};
pub use resolver::{ContextResolver, JsonLdError, ResolvedContext};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_core_context_expansion_compaction() {
        let resolver = ContextResolver::new(100);
        let ctx_val = json!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld");
        let resolved = resolver.resolve(&ctx_val).await.unwrap();

        assert_eq!(
            resolved.expand("Property"),
            "https://uri.etsi.org/ngsi-ld/Property"
        );
        assert_eq!(
            resolved.expand("value"),
            "https://uri.etsi.org/ngsi-ld/hasValue"
        );
        assert_eq!(
            resolved.compact("https://uri.etsi.org/ngsi-ld/Property"),
            "Property"
        );
        assert_eq!(
            resolved.compact("https://uri.etsi.org/ngsi-ld/hasValue"),
            "value"
        );
    }

    #[tokio::test]
    async fn test_custom_context_expansion() {
        let resolver = ContextResolver::new(100);
        let ctx_val = json!({
            "@context": {
                "schema": "https://schema.org/",
                "speed": "https://schema.org/speed"
            }
        });
        let resolved = resolver.resolve(&ctx_val).await.unwrap();

        assert_eq!(resolved.expand("speed"), "https://schema.org/speed");
        assert_eq!(resolved.expand("schema:name"), "https://schema.org/name");
        assert_eq!(resolved.compact("https://schema.org/speed"), "speed");
    }
}

pub mod processor;
pub use processor::Processor;
