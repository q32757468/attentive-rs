use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{error::Error, fmt};
use url::Url;

pub const API_VERSION: &str = "v1";
pub const NOTIFICATIONS_PATH: &str = "/api/v1/notifications";
pub const HEALTH_PATH: &str = "/health";

pub const DEFAULT_NOTIFIER_HOST: &str = "0.0.0.0";
pub const DEFAULT_NOTIFIER_PORT: u16 = 8765;
pub const DEFAULT_NOTIFIER_URL: &str = "http://127.0.0.1:8765";

pub const MAX_OPEN_URI_LENGTH: usize = 4096;
pub const OPEN_URI_SCHEMES: [&str; 3] = ["http:", "https:", "vscode:"];

pub type JsonObject = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenUriAction {
    #[serde(rename = "type")]
    action_type: OpenUriActionType,
    pub uri: String,
}

impl OpenUriAction {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            action_type: OpenUriActionType::OpenUri,
            uri: uri.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum OpenUriActionType {
    #[serde(rename = "open-uri")]
    OpenUri,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationRequest {
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<OpenUriAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonObject>,
}

impl NotificationRequest {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            source: None,
            action: None,
            metadata: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateNotificationResponse {
    pub notification_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidRequest,
    NotFound,
    MethodNotAllowed,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidNotificationRequestError {
    pub message: String,
}

impl InvalidNotificationRequestError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for InvalidNotificationRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for InvalidNotificationRequestError {}

pub fn create_api_error(code: ErrorCode, message: impl Into<String>) -> ApiErrorResponse {
    ApiErrorResponse {
        error: ApiError {
            code,
            message: message.into(),
        },
    }
}

pub fn is_json_object(value: &Value) -> bool {
    value.is_object()
}

/// Validates and normalizes an incoming protocol request.
///
/// Unknown properties are ignored, matching the TypeScript protocol package.
pub fn validate_notification_request(
    value: Value,
) -> Result<NotificationRequest, InvalidNotificationRequestError> {
    let object = value.as_object().ok_or_else(|| {
        InvalidNotificationRequestError::new("request body must be a JSON object")
    })?;

    let title = require_non_empty_string(object.get("title"), "title")?;
    let body = require_non_empty_string(object.get("body"), "body")?;
    let source = object
        .get("source")
        .map(|value| require_string(Some(value), "source"))
        .transpose()?;

    if object.contains_key("url") {
        return Err(InvalidNotificationRequestError::new(
            "url is no longer supported; use action",
        ));
    }

    let action = object.get("action").map(validate_action).transpose()?;
    let metadata = object
        .get("metadata")
        .map(|value| {
            value.as_object().cloned().ok_or_else(|| {
                InvalidNotificationRequestError::new("metadata must be a JSON object")
            })
        })
        .transpose()?;

    Ok(NotificationRequest {
        title,
        body,
        source,
        action,
        metadata,
    })
}

pub fn is_http_url(value: &str) -> bool {
    Url::parse(value)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

pub fn is_open_uri(value: &str) -> bool {
    if value.is_empty() || value.encode_utf16().count() > MAX_OPEN_URI_LENGTH {
        return false;
    }

    Url::parse(value)
        .map(|uri| matches!(uri.scheme(), "http" | "https" | "vscode"))
        .unwrap_or(false)
}

fn validate_action(value: &Value) -> Result<OpenUriAction, InvalidNotificationRequestError> {
    let object = value
        .as_object()
        .ok_or_else(|| InvalidNotificationRequestError::new("action must be a JSON object"))?;

    if object.get("type") != Some(&Value::String("open-uri".to_string())) {
        return Err(InvalidNotificationRequestError::new(
            "action.type must be \"open-uri\"",
        ));
    }

    let uri = require_string(object.get("uri"), "action.uri")?;
    if !is_open_uri(&uri) {
        return Err(InvalidNotificationRequestError::new(format!(
            "action.uri must use http, https, or vscode and be at most {MAX_OPEN_URI_LENGTH} characters"
        )));
    }

    Ok(OpenUriAction::new(uri))
}

fn require_non_empty_string(
    value: Option<&Value>,
    field: &str,
) -> Result<String, InvalidNotificationRequestError> {
    let result = require_string(value, field)?;
    if result.trim().is_empty() {
        return Err(InvalidNotificationRequestError::new(format!(
            "{field} is required"
        )));
    }
    Ok(result)
}

fn require_string(
    value: Option<&Value>,
    field: &str,
) -> Result<String, InvalidNotificationRequestError> {
    match value {
        None => Err(InvalidNotificationRequestError::new(format!(
            "{field} is required"
        ))),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(InvalidNotificationRequestError::new(format!(
            "{field} must be a string"
        ))),
    }
}
