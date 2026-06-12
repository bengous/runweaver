use std::sync::Arc;

use serde_json::Value;

use crate::services::RunweaverServices;

/// An operation body: JSON input plus injected services, JSON output or
/// [`OperationError`].
pub type OperationExecuteFn = Arc<
    dyn for<'a> Fn(Value, &RunweaverServices<'a>) -> Result<Value, OperationError>
        + Send
        + Sync
        + 'static,
>;

/// A named-by-registration unit of work: a closure from JSON input and
/// [`RunweaverServices`] to JSON output. Operations are what bindings route
/// surface events to.
#[derive(Clone)]
pub struct OperationDefinition {
    pub description: Option<String>,
    pub execute: OperationExecuteFn,
}

impl OperationDefinition {
    pub fn new(
        execute: impl for<'a> Fn(Value, &RunweaverServices<'a>) -> Result<Value, OperationError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            description: None,
            execute: Arc::new(execute),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl std::fmt::Debug for OperationDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationDefinition")
            .field("description", &self.description)
            .field("execute", &"<fn>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct OperationError {
    pub message: String,
}

impl OperationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Identity function marking a value as a complete operation definition.
pub fn define_operation(definition: OperationDefinition) -> OperationDefinition {
    definition
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::TestPorts;

    #[test]
    fn define_operation_preserves_description_and_executor() {
        let operation = define_operation(
            OperationDefinition::new(|input, _services| {
                let count = input
                    .get("files")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                Ok(serde_json::json!({ "count": count }))
            })
            .with_description("Count files"),
        );
        let ports = TestPorts::default();
        let services = ports.services();

        let output =
            (operation.execute)(serde_json::json!({ "files": ["a.ts", "b.ts"] }), &services)
                .unwrap();

        assert_eq!(operation.description.as_deref(), Some("Count files"));
        assert_eq!(output, serde_json::json!({ "count": 2 }));
    }
}
