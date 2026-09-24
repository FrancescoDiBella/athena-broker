use crate::error::ProblemDetails;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchEntityError {
    #[serde(rename = "entityId")]
    pub entity_id: String,
    pub error: ProblemDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BatchOperationResult {
    pub success: Vec<String>,
    pub errors: Vec<BatchEntityError>,
}

impl BatchOperationResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_success(&mut self, entity_id: impl Into<String>) {
        self.success.push(entity_id.into());
    }

    pub fn add_error(&mut self, entity_id: impl Into<String>, error: ProblemDetails) {
        self.errors.push(BatchEntityError {
            entity_id: entity_id.into(),
            error,
        });
    }

    pub fn is_all_success(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn is_all_failed(&self) -> bool {
        self.success.is_empty() && !self.errors.is_empty()
    }

    pub fn is_partial(&self) -> bool {
        !self.success.is_empty() && !self.errors.is_empty()
    }
}
