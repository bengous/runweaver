use serde_json::Value;
use thiserror::Error;

use super::contract::HookStage;

#[derive(Debug, Error)]
pub enum HookError {
    #[error("{0} hook payload is empty.")]
    EmptyPayload(&'static str),
    #[error("{harness} hook payload must be a JSON object.")]
    NonObjectPayload { harness: &'static str },
    #[error("{0}")]
    Contract(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn parse_payload(
    stdin: &str,
    harness: &'static str,
) -> Result<serde_json::Map<String, Value>, HookError> {
    if stdin.trim().is_empty() {
        return Err(HookError::EmptyPayload(harness));
    }
    match serde_json::from_str::<Value>(stdin)? {
        Value::Object(object) => Ok(object),
        _ => Err(HookError::NonObjectPayload { harness }),
    }
}

pub fn require_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
    harness: &'static str,
) -> Result<String, HookError> {
    match object.get(field) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(_) => Err(HookError::Contract(format!(
            "{harness} hook payload field {field} must be a non-empty string."
        ))),
        None => Err(HookError::Contract(format!(
            "{harness} hook payload is missing required field {field}."
        ))),
    }
}

pub fn require_object<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
    harness: &'static str,
) -> Result<&'a serde_json::Map<String, Value>, HookError> {
    match object.get(field) {
        Some(Value::Object(value)) => Ok(value),
        Some(_) => Err(HookError::Contract(format!(
            "{harness} hook payload field {field} must be an object."
        ))),
        None => Err(HookError::Contract(format!(
            "{harness} hook payload is missing required field {field}."
        ))),
    }
}

pub fn require_present_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
    harness: &'static str,
) -> Result<Value, HookError> {
    object.get(field).cloned().ok_or_else(|| {
        HookError::Contract(format!(
            "{harness} hook payload is missing required field {field}."
        ))
    })
}

pub fn optional_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
    harness: &'static str,
) -> Result<Option<String>, HookError> {
    match object.get(field) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(HookError::Contract(format!(
            "{harness} hook payload field {field} must be a non-empty string."
        ))),
        None => Ok(None),
    }
}

pub fn optional_bool(
    object: &serde_json::Map<String, Value>,
    field: &str,
    harness: &'static str,
) -> Result<Option<bool>, HookError> {
    match object.get(field) {
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(HookError::Contract(format!(
            "{harness} hook payload field {field} must be a boolean."
        ))),
        None => Ok(None),
    }
}

pub fn require_event_name(
    object: &serde_json::Map<String, Value>,
    harness: &'static str,
    stage: HookStage,
) -> Result<(), HookError> {
    let actual = require_string(object, "hook_event_name", harness)?;
    let expected = stage.expected_pi_event_name();
    if actual == expected {
        return Ok(());
    }
    Err(HookError::Contract(format!(
        "{harness} hook payload field hook_event_name must be {expected:?} for {stage:?}; received {actual:?}."
    )))
}
