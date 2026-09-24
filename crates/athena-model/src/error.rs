use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ERROR_BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";
pub const ERROR_INVALID_REQUEST: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";
pub const ERROR_RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";
pub const ERROR_ALREADY_EXISTS: &str = "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists";
pub const ERROR_OPERATION_NOT_SUPPORTED: &str =
    "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported";
pub const ERROR_INTERNAL_ERROR: &str = "https://uri.etsi.org/ngsi-ld/errors/InternalError";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProblemDetails {
    #[serde(rename = "type")]
    pub r#type: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl ProblemDetails {
    pub fn new(
        type_uri: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
        status: u16,
    ) -> Self {
        Self {
            r#type: type_uri.into(),
            title: title.into(),
            detail: detail.into(),
            status: Some(status),
        }
    }

    pub fn bad_request_data(detail: impl Into<String>) -> Self {
        Self::new(ERROR_BAD_REQUEST_DATA, "Bad Request Data", detail, 400)
    }

    pub fn invalid_request(detail: impl Into<String>) -> Self {
        Self::new(ERROR_INVALID_REQUEST, "Invalid Request", detail, 400)
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(ERROR_RESOURCE_NOT_FOUND, "Resource Not Found", detail, 404)
    }

    pub fn already_exists(detail: impl Into<String>) -> Self {
        Self::new(ERROR_ALREADY_EXISTS, "Already Exists", detail, 409)
    }

    pub fn operation_not_supported(detail: impl Into<String>) -> Self {
        Self::new(
            ERROR_OPERATION_NOT_SUPPORTED,
            "Operation Not Supported",
            detail,
            422,
        )
    }

    pub fn internal_error(detail: impl Into<String>) -> Self {
        Self::new(ERROR_INTERNAL_ERROR, "Internal Error", detail, 500)
    }
}

impl std::fmt::Display for ProblemDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.title, self.detail)
    }
}

impl std::error::Error for ProblemDetails {}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("Missing mandatory field: {0}")]
    MissingField(&'static str),

    #[error("Invalid URI format for field '{field}': '{value}'")]
    InvalidUri { field: &'static str, value: String },

    #[error("Invalid entity type: {0}")]
    InvalidType(String),

    #[error("Invalid attribute structure for '{0}'")]
    InvalidAttribute(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error(transparent)]
    Problem(#[from] ProblemDetails),
}
