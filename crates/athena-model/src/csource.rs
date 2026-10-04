use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::geoproperty::Geometry;
use crate::subscription::EntityInfo;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistrationStatus {
    Active,
    Paused,
    Expired,
}

impl Default for RegistrationStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistrationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntityInfo>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "propertyNames")]
    pub property_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "relationshipNames")]
    pub relationship_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeInterval {
    pub start: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CsourceRegistration {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String, // "ContextSourceRegistration"
    #[serde(skip_serializing_if = "Option::is_none", rename = "registrationName")]
    pub registration_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub information: Vec<RegistrationInfo>,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "contextSourceInfo")]
    pub context_source_info: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Geometry>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "observationSpace")]
    pub observation_space: Option<Geometry>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "operationSpace")]
    pub operation_space: Option<Geometry>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: RegistrationStatus,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "@context")]
    pub context: Option<serde_json::Value>,
}

impl CsourceRegistration {
    pub fn new(
        id: impl Into<String>,
        endpoint: impl Into<String>,
        information: Vec<RegistrationInfo>,
    ) -> Self {
        Self {
            id: id.into(),
            r#type: "ContextSourceRegistration".to_string(),
            registration_name: None,
            description: None,
            information,
            endpoint: endpoint.into(),
            context_source_info: None,
            location: None,
            observation_space: None,
            operation_space: None,
            expires_at: None,
            status: RegistrationStatus::Active,
            created_at: Some(Utc::now()),
            modified_at: Some(Utc::now()),
            context: None,
        }
    }

    pub fn matches_entity_query(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        attrs: Option<&[String]>,
    ) -> bool {
        if self.status != RegistrationStatus::Active {
            return false;
        }

        if let Some(exp) = self.expires_at {
            if exp <= Utc::now() {
                return false;
            }
        }

        // If information is empty, it registers all entities
        if self.information.is_empty() {
            return true;
        }

        for info in &self.information {
            // Check entities filter
            let entity_matches = if let Some(entities) = &info.entities {
                if entities.is_empty() {
                    true
                } else {
                    entities.iter().any(|target| {
                        if let Some(et) = entity_type {
                            if !et.split(',').any(|kind| target.r#type == kind) {
                                return false;
                            }
                        }
                        if let Some(eid) = entity_id {
                            if let Some(tid) = &target.id {
                                if !eid.split(',').any(|id| tid == id) {
                                    return false;
                                }
                            }
                            if let Some(pat) = &target.id_pattern {
                                if let Ok(re) = regex::Regex::new(pat) {
                                    if !eid.split(',').any(|id| re.is_match(id)) {
                                        return false;
                                    }
                                }
                            }
                        }
                        true
                    })
                }
            } else {
                true
            };

            if !entity_matches {
                continue;
            }

            // Check attributes filter if query specified attrs
            if let Some(queried_attrs) = attrs {
                if !queried_attrs.is_empty() {
                    let mut has_matching_attr = false;
                    if let Some(props) = &info.property_names {
                        if queried_attrs.iter().any(|qa| props.contains(qa)) {
                            has_matching_attr = true;
                        }
                    }
                    if let Some(rels) = &info.relationship_names {
                        if queried_attrs.iter().any(|qa| rels.contains(qa)) {
                            has_matching_attr = true;
                        }
                    }
                    if info.property_names.is_none() && info.relationship_names.is_none() {
                        has_matching_attr = true;
                    }
                    if !has_matching_attr {
                        continue;
                    }
                }
            }

            return true;
        }

        false
    }
}
