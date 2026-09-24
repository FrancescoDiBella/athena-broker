use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub r#type: String, // Always "Relationship"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "observedAt")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub sub_attributes: BTreeMap<String, serde_json::Value>,
}

impl Relationship {
    pub fn new(object_uri: impl Into<String>) -> Self {
        Self {
            r#type: "Relationship".to_string(),
            object: Some(object_uri.into()),
            objects: None,
            observed_at: None,
            dataset_id: None,
            created_at: None,
            modified_at: None,
            sub_attributes: BTreeMap::new(),
        }
    }

    pub fn new_multi(object_uris: Vec<String>) -> Self {
        Self {
            r#type: "Relationship".to_string(),
            object: None,
            objects: Some(object_uris),
            observed_at: None,
            dataset_id: None,
            created_at: None,
            modified_at: None,
            sub_attributes: BTreeMap::new(),
        }
    }
}
