use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Property {
    #[serde(rename = "type")]
    pub r#type: String, // Always "Property"
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none", rename = "observedAt")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "unitCode")]
    pub unit_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub sub_attributes: BTreeMap<String, serde_json::Value>,
}

impl Property {
    pub fn new(value: impl Into<serde_json::Value>) -> Self {
        Self {
            r#type: "Property".to_string(),
            value: value.into(),
            observed_at: None,
            unit_code: None,
            dataset_id: None,
            created_at: None,
            modified_at: None,
            sub_attributes: BTreeMap::new(),
        }
    }

    pub fn with_unit_code(mut self, code: impl Into<String>) -> Self {
        self.unit_code = Some(code.into());
        self
    }

    pub fn with_observed_at(mut self, dt: DateTime<Utc>) -> Self {
        self.observed_at = Some(dt);
        self
    }

    pub fn with_dataset_id(mut self, id: impl Into<String>) -> Self {
        self.dataset_id = Some(id.into());
        self
    }
}
