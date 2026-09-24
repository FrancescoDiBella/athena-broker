use crate::{Geometry, ModelError, ProblemDetails};
use serde_json::{Map, Value};
use std::collections::HashSet;

pub fn valid_uri(value: &str) -> bool {
    !value.chars().any(|c| c.is_whitespace() || c.is_control()) && url::Url::parse(value).is_ok()
}

fn bad(message: impl Into<String>) -> ModelError {
    ProblemDetails::bad_request_data(message).into()
}

pub fn validate_attributes(attributes: &Map<String, Value>) -> Result<(), ModelError> {
    if attributes.len() > 1000 {
        return Err(bad("Too many attributes"));
    }
    for (name, value) in attributes {
        if name.is_empty()
            || name.starts_with('@')
            || matches!(
                name.as_str(),
                "id" | "type" | "scope" | "createdAt" | "modifiedAt"
            )
        {
            return Err(bad(format!("Reserved or invalid attribute name '{name}'")));
        }
        validate_attribute(name, value, 0)?;
    }
    Ok(())
}

pub fn validate_attribute(name: &str, value: &Value, depth: usize) -> Result<(), ModelError> {
    if depth > 32 {
        return Err(bad("Attribute nesting exceeds 32 levels"));
    }
    if let Some(instances) = value.as_array() {
        if instances.is_empty() || instances.len() > 1000 {
            return Err(bad("Invalid number of attribute instances"));
        }
        let mut datasets = HashSet::new();
        for instance in instances {
            if !instance.is_object() {
                return Err(bad("An attribute instance must be an object"));
            }
            let dataset = instance
                .get("datasetId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !datasets.insert(dataset) {
                return Err(bad(format!("Duplicate datasetId in '{name}'")));
            }
            validate_attribute(name, instance, depth + 1)?;
        }
        return Ok(());
    }
    let object = value
        .as_object()
        .ok_or_else(|| bad(format!("Attribute '{name}' must be an object")))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("Missing type on attribute '{name}'")))?;
    let value_key = match kind {
        "Property" | "GeoProperty" => "value",
        "Relationship" => {
            if object.contains_key("object") {
                "object"
            } else {
                "objects"
            }
        }
        "LanguageProperty" => "languageMap",
        "VocabProperty" => "vocab",
        "JsonProperty" => "json",
        "ListProperty" => "valueList",
        "ListRelationship" => "objectList",
        _ => return Err(bad(format!("Unsupported attribute type '{kind}'"))),
    };
    let content = object
        .get(value_key)
        .ok_or_else(|| bad(format!("Missing '{value_key}' on '{name}'")))?;
    if kind != "JsonProperty" && content.is_null() {
        return Err(bad("Attribute value cannot be null"));
    }
    if kind == "GeoProperty" {
        let geometry: Geometry =
            serde_json::from_value(content.clone()).map_err(|_| bad("Invalid GeoJSON geometry"))?;
        geometry.validate().map_err(bad)?;
    }
    if kind == "Relationship" || kind == "ListRelationship" {
        let valid = content.as_str().is_some_and(valid_uri)
            || content.as_array().is_some_and(|a| {
                !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(valid_uri))
            });
        if !valid {
            return Err(bad("Relationship objects must be absolute URIs"));
        }
    }
    if matches!(kind, "ListProperty" | "ListRelationship") && !content.is_array() {
        return Err(bad("List attributes require an array"));
    }
    if kind == "LanguageProperty"
        && !content.as_object().is_some_and(|m| {
            m.values().all(|v| {
                v.is_string() || v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
            })
        })
    {
        return Err(bad("Invalid languageMap"));
    }
    for (key, member) in object {
        match key.as_str() {
            "type" => {}
            k if k == value_key => {}
            "datasetId" | "instanceId" => {
                if !member.as_str().is_some_and(valid_uri) {
                    return Err(bad(format!("'{key}' must be an absolute URI")));
                }
            }
            "observedAt" | "createdAt" | "modifiedAt" | "deletedAt" => {
                if !member
                    .as_str()
                    .is_some_and(|s| chrono::DateTime::parse_from_rfc3339(s).is_ok())
                {
                    return Err(bad(format!("Invalid timestamp '{key}'")));
                }
            }
            "unitCode" => {
                if !member.is_string() {
                    return Err(bad("unitCode must be a string"));
                }
            }
            "objectType" => {
                if !member.is_string() && !member.is_array() {
                    return Err(bad("Invalid objectType"));
                }
            }
            _ => validate_attribute(key, member, depth + 1)?,
        }
    }
    Ok(())
}

pub fn without_context(value: &Value) -> Result<Map<String, Value>, ModelError> {
    let mut attrs = value
        .as_object()
        .cloned()
        .ok_or_else(|| bad("Attributes must be an object"))?;
    attrs.remove("@context");
    validate_attributes(&attrs)?;
    Ok(attrs)
}

pub fn simplified(value: &Value) -> Value {
    if let Some(instances) = value.as_array() {
        return Value::Array(instances.iter().map(simplified).collect());
    }
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    for key in [
        "value",
        "object",
        "objects",
        "languageMap",
        "vocab",
        "json",
        "valueList",
        "objectList",
    ] {
        if let Some(value) = object.get(key) {
            return value.clone();
        }
    }
    value.clone()
}

/// Merge whole instances by dataset identity; never overwrite other datasets.
pub fn merge_instances(old: &Value, incoming: &Value, overwrite: bool) -> Value {
    let mut values = old.as_array().cloned().unwrap_or_else(|| vec![old.clone()]);
    let additions = incoming
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![incoming.clone()]);
    for addition in additions {
        let dataset = addition.get("datasetId");
        if let Some(index) = values.iter().position(|v| v.get("datasetId") == dataset) {
            if overwrite {
                values[index] = addition;
            }
        } else {
            values.push(addition);
        }
    }
    if values.len() == 1 {
        values.remove(0)
    } else {
        Value::Array(values)
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rejects_duplicate_dataset_identity_and_invalid_geometry() {
        assert!(validate_attribute(
            "t",
            &json!([{"type":"Property","value":1},{"type":"Property","value":2}]),
            0
        )
        .is_err());
        assert!(validate_attribute("t",&json!([{"type":"Property","value":1,"datasetId":"urn:ds:a"},{"type":"Property","value":2,"datasetId":"urn:ds:b"}]),0).is_ok());
        assert!(validate_attribute(
            "location",
            &json!({"type":"GeoProperty","value":{"type":"Point","coordinates":[181,91]}}),
            0
        )
        .is_err());
        assert!(validate_attribute("location",&json!({"type":"GeoProperty","value":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1]]]}}),0).is_err());
    }
    #[test]
    fn preserves_json_and_list_values() {
        let value = json!({"type":"JsonProperty","json":{"id":"literal","nested":[1,2]}});
        assert!(validate_attribute("payload", &value, 0).is_ok());
        assert_eq!(simplified(&value), value["json"]);
        let list = json!({"type":"ListRelationship","objectList":["urn:a","urn:b"]});
        assert!(validate_attribute("members", &list, 0).is_ok());
        assert_eq!(simplified(&list), json!(["urn:a", "urn:b"]));
    }
}
