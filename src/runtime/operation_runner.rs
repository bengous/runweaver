use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bindings::{
    Binding, BindingResolution, BindingRunError, BoundOperationRunResult, run_bound_operation,
    run_resolved_binding,
};
use crate::config::{ExecutionContext, RunweaverDefinition, RunweaverOperationDefinition, TaskRun};
use crate::services::RunweaverServices;

use super::run_task;

/// Output of running a registered operation: a plain JSON value for closure
/// operations, or the [`TaskRun`] tree for task-backed ones. Use
/// [`into_json_output`](Self::into_json_output) to flatten either to JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunweaverOperationRunResult {
    Operation { output: Value },
    Task { run: Box<TaskRun> },
}

impl RunweaverOperationRunResult {
    pub fn into_json_output(self) -> Result<Value, RunweaverOperationRunError> {
        match self {
            Self::Operation { output } => Ok(output),
            Self::Task { run } => serde_json::to_value(run).map_err(|error| {
                RunweaverOperationRunError::ResultSerialization {
                    message: error.to_string(),
                }
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunweaverOperationRunError {
    #[error("Runweaver operation \"{operation_name}\" does not exist.")]
    MissingOperation { operation_name: String },
    #[error("Runweaver operation \"{operation_name}\" failed: {message}")]
    Operation {
        operation_name: String,
        message: String,
    },
    #[error("Runweaver task operation \"{operation_name}\" failed: {message}")]
    Task {
        operation_name: String,
        message: String,
    },
    #[error("Runweaver operation result could not be converted to JSON: {message}")]
    ResultSerialization { message: String },
}

/// Runs the named operation from a definition with the given JSON input,
/// execution context (for task-backed operations), and services.
pub fn run_runweaver_operation(
    definition: &RunweaverDefinition,
    operation_name: &str,
    input: Value,
    execution_context: ExecutionContext,
    services: &RunweaverServices<'_>,
) -> Result<RunweaverOperationRunResult, RunweaverOperationRunError> {
    let operation = definition.operations.get(operation_name).ok_or_else(|| {
        RunweaverOperationRunError::MissingOperation {
            operation_name: operation_name.to_owned(),
        }
    })?;

    match operation {
        RunweaverOperationDefinition::Operation(operation) => (operation.execute)(input, services)
            .map(|output| RunweaverOperationRunResult::Operation { output })
            .map_err(|error| RunweaverOperationRunError::Operation {
                operation_name: operation_name.to_owned(),
                message: error.message,
            }),
        RunweaverOperationDefinition::Task(task) => {
            let mut config = definition.task_config();
            config.tasks.insert(operation_name.to_owned(), task.clone());
            let task_context = execution_context.with_input(input);
            let run = run_task(&config, operation_name, task_context).map_err(|error| {
                RunweaverOperationRunError::Task {
                    operation_name: operation_name.to_owned(),
                    message: error.to_string(),
                }
            })?;
            Ok(RunweaverOperationRunResult::Task { run: Box::new(run) })
        }
    }
}

pub fn run_runweaver_operation_as_json(
    definition: &RunweaverDefinition,
    operation_name: &str,
    input: Value,
    execution_context: ExecutionContext,
    services: &RunweaverServices<'_>,
) -> Result<Value, RunweaverOperationRunError> {
    run_runweaver_operation(
        definition,
        operation_name,
        input,
        execution_context,
        services,
    )?
    .into_json_output()
}

pub fn run_bound_runweaver_operation(
    definition: &RunweaverDefinition,
    binding: &Binding,
    input: Value,
    binding_context: &mut Value,
    execution_context: ExecutionContext,
    services: &RunweaverServices<'_>,
) -> Result<Value, BindingRunError> {
    run_bound_operation(
        binding,
        &|operation_name, input, _binding_context| {
            run_runweaver_operation_as_json(
                definition,
                operation_name,
                input,
                execution_context.clone(),
                services,
            )
            .map_err(|error| BindingRunError::operation(error.to_string()))
        },
        input,
        binding_context,
    )
}

pub fn run_resolved_runweaver_binding(
    definition: &RunweaverDefinition,
    resolution: &BindingResolution,
    input: Value,
    binding_context: &mut Value,
    execution_context: ExecutionContext,
    services: &RunweaverServices<'_>,
) -> Result<BoundOperationRunResult, BindingRunError> {
    run_resolved_binding(
        resolution,
        &|operation_name, input, _binding_context| {
            run_runweaver_operation_as_json(
                definition,
                operation_name,
                input,
                execution_context.clone(),
                services,
            )
            .map_err(|error| BindingRunError::operation(error.to_string()))
        },
        input,
        binding_context,
    )
}

#[cfg(test)]
mod tests {
    use crate::bindings::bind;
    use crate::config::{ActionResult, ActionTask, TaskCompletion, TaskDefinition};
    use crate::core::{OperationDefinition, OperationError};
    use crate::services::test_support::TestPorts;
    use crate::surfaces::SurfaceTrigger;

    use super::*;

    fn trigger() -> SurfaceTrigger {
        SurfaceTrigger {
            surface: "cli".to_owned(),
            name: "count".to_owned(),
            phase: None,
        }
    }

    #[test]
    fn run_runweaver_operation_executes_operation_definitions() {
        let definition = RunweaverDefinition::new().with_operation(
            "countFiles",
            OperationDefinition::new(|input, _services| {
                let count = input
                    .get("files")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                Ok(serde_json::json!({ "count": count }))
            }),
        );
        let ports = TestPorts::default();

        let result = run_runweaver_operation(
            &definition,
            "countFiles",
            serde_json::json!({ "files": ["a.rs", "b.rs"] }),
            ExecutionContext::new("."),
            &ports.services(),
        )
        .unwrap();

        assert_eq!(
            result,
            RunweaverOperationRunResult::Operation {
                output: serde_json::json!({ "count": 2 })
            }
        );
    }

    #[test]
    fn run_runweaver_operation_executes_task_operations_with_input_context() {
        let definition = RunweaverDefinition::new().with_operation(
            "echoInput",
            TaskDefinition::Action(ActionTask::new(|ctx| {
                ActionResult::completed()
                    .completion(TaskCompletion::Success)
                    .data(ctx.input.clone().unwrap_or(Value::Null))
                    .build()
            })),
        );
        let ports = TestPorts::default();

        let result = run_runweaver_operation_as_json(
            &definition,
            "echoInput",
            serde_json::json!({ "value": 7 }),
            ExecutionContext::new("."),
            &ports.services(),
        )
        .unwrap();

        assert_eq!(result["data"], serde_json::json!({ "value": 7 }));
    }

    #[test]
    fn run_resolved_runweaver_binding_executes_definition_operations_with_profiles() {
        let definition = RunweaverDefinition::new().with_operation(
            "countFiles",
            OperationDefinition::new(|input, _services| {
                let count = input
                    .get("files")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                Ok(serde_json::json!({ "count": count }))
            }),
        );
        let binding = bind(trigger()).to("countFiles").finish();
        let resolution = BindingResolution::Matched {
            binding: binding.clone(),
        };
        let ports = TestPorts::default();
        let mut binding_context = serde_json::json!({});

        let result = run_resolved_runweaver_binding(
            &definition,
            &resolution,
            serde_json::json!({ "files": ["a.rs"] }),
            &mut binding_context,
            ExecutionContext::new("."),
            &ports.services(),
        )
        .unwrap();

        assert_eq!(
            result,
            BoundOperationRunResult::Executed {
                output: serde_json::json!({ "count": 1 })
            }
        );
    }

    #[test]
    fn run_bound_runweaver_operation_maps_operation_errors_to_binding_errors() {
        let definition = RunweaverDefinition::new().with_operation(
            "fail",
            OperationDefinition::new(|_input, _services| Err(OperationError::new("boom"))),
        );
        let binding = bind(trigger()).to("fail").finish();
        let ports = TestPorts::default();
        let mut binding_context = serde_json::json!({});

        let error = run_bound_runweaver_operation(
            &definition,
            &binding,
            serde_json::json!({}),
            &mut binding_context,
            ExecutionContext::new("."),
            &ports.services(),
        )
        .unwrap_err();

        assert_eq!(
            error,
            BindingRunError::operation("Runweaver operation \"fail\" failed: boom")
        );
    }
}
