//! Local entity mutation semantics, independent of HTTP and persistence.
use crate::{attributes, Entity, ProblemDetails};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const NGSI_NULL: &str = "urn:ngsi-ld:null";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeOperation {
    Update,
    Append { overwrite: bool },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdateResult {
    pub updated: Vec<String>,
    #[serde(rename = "notUpdated")]
    pub not_updated: Vec<NotUpdatedDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotUpdatedDetails {
    #[serde(rename = "attributeName")]
    pub attribute_name: String,
    pub reason: String,
}

fn bad(message: impl Into<String>) -> ProblemDetails {
    ProblemDetails::bad_request_data(message)
}

fn instances(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()])
}

fn set_instances(attrs: &mut Map<String, Value>, name: &str, mut values: Vec<Value>) {
    match values.len() {
        0 => {
            attrs.remove(name);
        }
        1 => {
            attrs.insert(name.into(), values.remove(0));
        }
        _ => {
            attrs.insert(name.into(), Value::Array(values));
        }
    }
}

fn deletion(value: &Value) -> bool {
    match value.get("type").and_then(Value::as_str) {
        Some("Property") => value.get("value").and_then(Value::as_str) == Some(NGSI_NULL),
        Some("Relationship") => value.get("object").and_then(Value::as_str) == Some(NGSI_NULL),
        Some("LanguageProperty") => {
            value.get("languageMap") == Some(&serde_json::json!({"@none":NGSI_NULL}))
        }
        _ => false,
    }
}

fn preserve_created_at(previous: &Value, incoming: &mut Value) {
    if let Some(object) = incoming.as_object_mut() {
        // System timestamps are controlled by the broker, never by a patch.
        object.remove("createdAt");
        object.remove("modifiedAt");
        if let Some(created) = previous.get("createdAt") {
            object.insert("createdAt".into(), created.clone());
        }
    }
}

/// Work on a copy so failed validation never exposes a partially mutated document.
pub fn apply_attributes(
    previous: &Value,
    fragment: &Value,
    operation: AttributeOperation,
) -> Result<(Value, UpdateResult), ProblemDetails> {
    let mut result = previous
        .as_object()
        .cloned()
        .ok_or_else(|| bad("Invalid stored attributes"))?;
    let clean = attributes::without_context(fragment).map_err(|e| bad(e.to_string()))?;
    let mut report = UpdateResult::default();
    for (name, incoming) in clean {
        if operation != AttributeOperation::Update && instances(&incoming).iter().any(deletion) {
            return Err(bad("NGSI-LD null deletion requires an update operation"));
        }
        let mut values = result.get(&name).map(instances).unwrap_or_default();
        let mut applied = false;
        let mut skipped = false;
        for mut addition in instances(&incoming) {
            let position = values
                .iter()
                .position(|v| v.get("datasetId") == addition.get("datasetId"));
            if operation == (AttributeOperation::Append { overwrite: false }) && position.is_some()
            {
                skipped = true;
                continue;
            }
            if deletion(&addition) {
                if operation != AttributeOperation::Update {
                    return Err(bad("NGSI-LD null deletion requires an update operation"));
                }
                if let Some(index) = position {
                    values.remove(index);
                }
            } else if let Some(index) = position {
                preserve_created_at(&values[index], &mut addition);
                values[index] = addition;
            } else {
                preserve_created_at(&Value::Null, &mut addition);
                values.push(addition);
            }
            applied = true;
        }
        set_instances(&mut result, &name, values);
        if applied {
            report.updated.push(name.clone());
        }
        if skipped {
            report.not_updated.push(NotUpdatedDetails {
                attribute_name: name,
                reason: "An existing dataset instance was preserved by noOverwrite".into(),
            });
        }
    }
    attributes::validate_attributes(&result).map_err(|e| bad(e.to_string()))?;
    Ok((Value::Object(result), report))
}

/// PATCH changes supplied subattributes; PUT replaces a complete existing instance.
pub fn apply_attribute(
    previous: &Value,
    name: &str,
    fragment: &Value,
    replace: bool,
) -> Result<Value, ProblemDetails> {
    let mut attrs = previous
        .as_object()
        .cloned()
        .ok_or_else(|| bad("Invalid stored attributes"))?;
    let mut patch = fragment
        .as_object()
        .cloned()
        .ok_or_else(|| bad("Attribute fragment must be an object"))?;
    patch.remove("@context");
    for key in ["createdAt", "modifiedAt", "deletedAt", "instanceId"] {
        patch.remove(key);
    }
    if let Some(dataset) = patch.get("datasetId") {
        if !dataset
            .as_str()
            .is_some_and(|s| s != NGSI_NULL && attributes::valid_uri(s))
        {
            return Err(bad(
                "datasetId must be an absolute URI and cannot be deleted",
            ));
        }
    }
    let mut values = attrs.get(name).map(instances).unwrap_or_default();
    let index = values
        .iter()
        .position(|v| v.get("datasetId") == patch.get("datasetId"))
        .ok_or_else(|| ProblemDetails::not_found("Attribute dataset instance not found"))?;
    let original = &values[index];
    if !replace
        && patch
            .get("type")
            .is_some_and(|kind| Some(kind) != original.get("type"))
    {
        return Err(bad("Partial attribute updates cannot change its type"));
    }
    let mut updated = if replace {
        Map::new()
    } else {
        original
            .as_object()
            .cloned()
            .ok_or_else(|| bad("Invalid stored attribute"))?
    };
    for (key, value) in patch {
        if value.as_str() == Some(NGSI_NULL) && !replace {
            if matches!(
                key.as_str(),
                "type"
                    | "value"
                    | "object"
                    | "languageMap"
                    | "json"
                    | "valueList"
                    | "objectList"
                    | "vocab"
            ) {
                return Err(bad(
                    "Partial updates cannot delete an attribute's required members",
                ));
            }
            updated.remove(&key);
        } else {
            updated.insert(key, value);
        }
    }
    let mut updated = Value::Object(updated);
    if deletion(&updated) {
        return Err(bad(
            "Use update attributes or DELETE to delete a whole attribute",
        ));
    }
    preserve_created_at(original, &mut updated);
    attributes::validate_attribute(name, &updated, 0).map_err(|e| bad(e.to_string()))?;
    values[index] = updated;
    set_instances(&mut attrs, name, values);
    Ok(Value::Object(attrs))
}

/// Validate entity identity when replacing an entity through its resource URI.
pub fn replacement(id: &str, payload: &Value) -> Result<Entity, ProblemDetails> {
    let mut document = payload
        .as_object()
        .cloned()
        .ok_or_else(|| bad("Entity must be an object"))?;
    if document
        .get("id")
        .is_some_and(|value| value.as_str() != Some(id))
    {
        return Err(bad("Payload id must match the resource URI"));
    }
    document.insert("id".into(), Value::String(id.into()));
    Entity::from_json(Value::Object(document)).map_err(|e| bad(e.to_string()))
}
