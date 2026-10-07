use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::error::{ModelError, ProblemDetails};
use crate::geoproperty::GeoProperty;
use crate::property::Property;
use crate::relationship::Relationship;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "@context")]
    pub context: Option<Value>,
}

impl Entity {
    pub fn new(id: impl Into<String>, type_: impl Into<String>) -> Self {
        let t = type_.into();
        Self {
            id: id.into(),
            type_: t.clone(),
            types: vec![t],
            attributes: BTreeMap::new(),
            scope: None,
            created_at: None,
            modified_at: None,
            context: None,
        }
    }

    pub fn with_context(mut self, context: Value) -> Self {
        self.context = Some(context);
        self
    }

    pub fn with_scope(mut self, scope: Vec<String>) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn from_json(val: Value) -> Result<Self, ModelError> {
        let obj = val.as_object().ok_or_else(|| {
            ModelError::Problem(ProblemDetails::bad_request_data(
                "Entity JSON payload must be an object",
            ))
        })?;

        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ModelError::MissingField("id"))?
            .to_string();

        if !crate::attributes::valid_uri(&id) {
            return Err(ModelError::Problem(ProblemDetails::bad_request_data(
                format!("Entity id '{id}' must be a valid URI (e.g. URN or URL)"),
            )));
        }

        let (type_, types) = match obj.get("type") {
            Some(Value::String(s)) if !s.is_empty() => (s.clone(), vec![s.clone()]),
            Some(Value::Array(arr)) => {
                let list: Vec<String> = arr
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect();
                if list.is_empty() || list.len() != arr.len() || list.iter().any(String::is_empty) {
                    return Err(ModelError::Problem(ProblemDetails::bad_request_data(
                        "Entity 'type' array cannot be empty",
                    )));
                }
                (list[0].clone(), list)
            }
            _ => return Err(ModelError::MissingField("type")),
        };

        let context = obj.get("@context").cloned();

        let scope = obj.get("scope").map(parse_scope).transpose()?;

        let created_at = obj
            .get("createdAt")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let modified_at = obj
            .get("modifiedAt")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let mut attributes = BTreeMap::new();
        for (k, v) in obj {
            if k == "id"
                || k == "type"
                || k == "@context"
                || k == "scope"
                || k == "createdAt"
                || k == "modifiedAt"
            {
                continue;
            }

            // In NGSI-LD normalized, each attribute MUST be an object containing "type"
            // or an array of attribute objects (multi-attribute instances)
            if v.is_object() || v.is_array() {
                attributes.insert(k.clone(), v.clone());
            } else {
                return Err(ModelError::Problem(ProblemDetails::bad_request_data(
                    format!("Attribute '{k}' must be a JSON object (Property, Relationship, or GeoProperty)"),
                )));
            }
        }

        crate::attributes::validate_attributes(
            &attributes
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )?;

        Ok(Self {
            id,
            type_,
            types,
            attributes,
            scope,
            created_at,
            modified_at,
            context,
        })
    }

    pub fn to_normalized(&self, include_sys_attrs: bool) -> Value {
        let mut map = Map::new();
        map.insert("id".to_string(), Value::String(self.id.clone()));

        if self.types.len() > 1 {
            let arr = self
                .types
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect();
            map.insert("type".to_string(), Value::Array(arr));
        } else {
            map.insert("type".to_string(), Value::String(self.type_.clone()));
        }

        if let Some(scope) = &self.scope {
            let arr = scope.iter().map(|s| Value::String(s.clone())).collect();
            map.insert("scope".to_string(), Value::Array(arr));
        }

        if include_sys_attrs {
            if let Some(created_at) = self.created_at {
                map.insert(
                    "createdAt".to_string(),
                    Value::String(created_at.to_rfc3339()),
                );
            }
            if let Some(modified_at) = self.modified_at {
                map.insert(
                    "modifiedAt".to_string(),
                    Value::String(modified_at.to_rfc3339()),
                );
            }
        }

        for (k, v) in &self.attributes {
            map.insert(k.clone(), v.clone());
        }

        if let Some(ctx) = &self.context {
            map.insert("@context".to_string(), ctx.clone());
        }

        Value::Object(map)
    }

    pub fn to_key_values(&self) -> Value {
        let mut map = Map::new();
        map.insert("id".to_string(), Value::String(self.id.clone()));

        if self.types.len() > 1 {
            let arr = self
                .types
                .iter()
                .map(|t| Value::String(t.clone()))
                .collect();
            map.insert("type".to_string(), Value::Array(arr));
        } else {
            map.insert("type".to_string(), Value::String(self.type_.clone()));
        }

        if let Some(scope) = &self.scope {
            let arr = scope.iter().map(|s| Value::String(s.clone())).collect();
            map.insert("scope".to_string(), Value::Array(arr));
        }

        for (k, v) in &self.attributes {
            map.insert(k.clone(), crate::attributes::simplified(v));
        }

        if let Some(ctx) = &self.context {
            map.insert("@context".to_string(), ctx.clone());
        }

        Value::Object(map)
    }

    pub fn set_property(&mut self, name: &str, prop: Property) -> Result<(), ModelError> {
        let val = serde_json::to_value(prop)?;
        self.attributes.insert(name.to_string(), val);
        Ok(())
    }

    pub fn set_relationship(&mut self, name: &str, rel: Relationship) -> Result<(), ModelError> {
        let val = serde_json::to_value(rel)?;
        self.attributes.insert(name.to_string(), val);
        Ok(())
    }

    pub fn set_geoproperty(&mut self, name: &str, geo: GeoProperty) -> Result<(), ModelError> {
        let val = serde_json::to_value(geo)?;
        self.attributes.insert(name.to_string(), val);
        Ok(())
    }

    pub fn get_property(&self, name: &str) -> Option<Property> {
        self.attributes
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn get_relationship(&self, name: &str) -> Option<Relationship> {
        self.attributes
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn get_geoproperty(&self, name: &str) -> Option<GeoProperty> {
        self.attributes
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

pub fn parse_scope(value: &Value) -> Result<Vec<String>, ModelError> {
    let values = value
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    if values.is_empty()
        || values.len() > 1000
        || values.iter().any(|v| {
            !v.as_str().is_some_and(|scope| {
                scope.starts_with('/')
                    && !scope.chars().any(|c| c.is_whitespace() || c.is_control())
            })
        })
    {
        return Err(ProblemDetails::bad_request_data(
            "scope requires nonempty absolute scope paths",
        )
        .into());
    }
    Ok(values
        .into_iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect())
}
