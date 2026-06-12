use std::collections::HashMap;

use crate::bindings::{Binding, BindingRegistry};
use crate::core::OperationDefinition;

use super::manifest::{RunweaverDefinitionManifest, create_runweaver_definition_manifest};
use super::validate::{RunweaverDefinitionValidation, validate_runweaver_definition};
use super::{PolicyDefinition, RunweaverConfig, TaskDefinition, ToolDefinition};

/// An entry in a definition's operation registry: either a plain
/// JSON-in/JSON-out [`OperationDefinition`] or a [`TaskDefinition`] promoted
/// to operation status (its run result becomes the operation output).
#[derive(Debug, Clone)]
pub enum RunweaverOperationDefinition {
    Operation(OperationDefinition),
    Task(TaskDefinition),
}

impl From<OperationDefinition> for RunweaverOperationDefinition {
    fn from(operation: OperationDefinition) -> Self {
        Self::Operation(operation)
    }
}

impl From<TaskDefinition> for RunweaverOperationDefinition {
    fn from(task: TaskDefinition) -> Self {
        Self::Task(task)
    }
}

pub type RunweaverOperationRegistry = HashMap<String, RunweaverOperationDefinition>;

/// The root aggregate of a project's automation: named tools, policies,
/// tasks, operations, and bindings.
///
/// Derive the task-runner view with [`task_config`](Self::task_config),
/// check consistency with [`validate`](Self::validate), and export the
/// serializable form with [`manifest`](Self::manifest).
#[derive(Debug, Clone, Default)]
pub struct RunweaverDefinition {
    pub tools: HashMap<String, ToolDefinition>,
    pub policies: HashMap<String, PolicyDefinition>,
    pub tasks: HashMap<String, TaskDefinition>,
    pub operations: RunweaverOperationRegistry,
    pub bindings: BindingRegistry,
}

impl RunweaverDefinition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tool(mut self, name: impl Into<String>, tool: ToolDefinition) -> Self {
        self.tools.insert(name.into(), tool);
        self
    }

    pub fn with_policy(mut self, name: impl Into<String>, policy: PolicyDefinition) -> Self {
        self.policies.insert(name.into(), policy);
        self
    }

    pub fn with_task(mut self, name: impl Into<String>, task: TaskDefinition) -> Self {
        self.tasks.insert(name.into(), task);
        self
    }

    pub fn with_operation(
        mut self,
        name: impl Into<String>,
        operation: impl Into<RunweaverOperationDefinition>,
    ) -> Self {
        self.operations.insert(name.into(), operation.into());
        self
    }

    pub fn with_binding(mut self, binding: Binding) -> Self {
        self.bindings.push(binding);
        self
    }

    pub fn task_config(&self) -> RunweaverConfig {
        RunweaverConfig {
            tools: self.tools.clone(),
            tasks: self.tasks.clone(),
            policies: self.policies.clone(),
        }
    }

    pub fn validate(&self) -> RunweaverDefinitionValidation {
        validate_runweaver_definition(self)
    }

    pub fn manifest(&self) -> RunweaverDefinitionManifest {
        create_runweaver_definition_manifest(self)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunweaverDefinitionBuilder {
    definition: RunweaverDefinition,
}

impl RunweaverDefinitionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tool(&mut self, name: impl Into<String>, tool: ToolDefinition) -> &mut Self {
        self.definition.tools.insert(name.into(), tool);
        self
    }

    pub fn policy(&mut self, name: impl Into<String>, policy: PolicyDefinition) -> &mut Self {
        self.definition.policies.insert(name.into(), policy);
        self
    }

    pub fn task(&mut self, name: impl Into<String>, task: impl Into<TaskDefinition>) -> &mut Self {
        self.definition.tasks.insert(name.into(), task.into());
        self
    }

    pub fn operation(
        &mut self,
        name: impl Into<String>,
        operation: impl Into<RunweaverOperationDefinition>,
    ) -> &mut Self {
        self.definition
            .operations
            .insert(name.into(), operation.into());
        self
    }

    pub fn binding(&mut self, binding: Binding) -> &mut Self {
        self.definition.bindings.push(binding);
        self
    }

    pub fn build(self) -> RunweaverDefinition {
        self.definition
    }
}

impl From<RunweaverConfig> for RunweaverDefinition {
    fn from(config: RunweaverConfig) -> Self {
        Self {
            tools: config.tools,
            policies: config.policies,
            tasks: config.tasks,
            operations: HashMap::new(),
            bindings: Vec::new(),
        }
    }
}

pub fn define_runweaver(definition: RunweaverDefinition) -> RunweaverDefinition {
    definition
}

pub fn define_runweaver_with(
    configure: impl FnOnce(&mut RunweaverDefinitionBuilder),
) -> RunweaverDefinition {
    let mut builder = RunweaverDefinitionBuilder::new();
    configure(&mut builder);
    define_runweaver(builder.build())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::bindings::bind;
    use crate::config::{
        CommandArgs, ExecutionContext, PolicyVerdict, command, host_command, policy,
    };
    use crate::core::OperationDefinition;
    use crate::surfaces::SurfaceTrigger;

    use super::*;

    fn allow_policy(_: &ExecutionContext) -> PolicyVerdict {
        PolicyVerdict::Allow
    }

    #[test]
    fn define_runweaver_preserves_tasks_operations_bindings_and_task_config() {
        let task = command("cargo", CommandArgs::Static(vec!["test".to_owned()]));
        let operation = OperationDefinition::new(|input, _services| {
            let count = input
                .get("files")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Ok(serde_json::json!({ "count": count }))
        })
        .with_description("Count files");
        let binding = bind(SurfaceTrigger {
            surface: "cli".to_owned(),
            name: "validate".to_owned(),
            phase: None,
        })
        .to("validate")
        .finish();

        let definition = define_runweaver(
            RunweaverDefinition::new()
                .with_tool("cargo", host_command("cargo"))
                .with_policy("allow", policy(allow_policy))
                .with_task("test", task.clone())
                .with_operation("validate", operation)
                .with_operation("test", task)
                .with_binding(binding),
        );
        let task_config = definition.task_config();

        assert!(definition.tools.contains_key("cargo"));
        assert!(definition.policies.contains_key("allow"));
        assert!(definition.tasks.contains_key("test"));
        assert_eq!(definition.bindings[0].operation_name, "validate");
        assert!(task_config.tasks.contains_key("test"));
        assert!(matches!(
            definition.operations.get("validate"),
            Some(RunweaverOperationDefinition::Operation(_))
        ));
        assert!(matches!(
            definition.operations.get("test"),
            Some(RunweaverOperationDefinition::Task(_))
        ));
    }

    #[test]
    fn runweaver_definition_can_be_created_from_task_config() {
        let mut config = RunweaverConfig::new();
        config
            .tools
            .insert("cargo".to_owned(), host_command("cargo"));
        config.tasks.insert(
            "test".to_owned(),
            command("cargo", CommandArgs::Static(vec!["test".to_owned()])),
        );

        let definition = RunweaverDefinition::from(config);

        assert!(definition.tools.contains_key("cargo"));
        assert!(definition.tasks.contains_key("test"));
        assert!(definition.operations.is_empty());
        assert!(definition.bindings.is_empty());
    }

    #[test]
    fn define_runweaver_with_builds_composition_without_manual_maps() {
        let definition = define_runweaver_with(|runweaver| {
            runweaver
                .tool("cargo", host_command("cargo"))
                .policy("allow", policy(allow_policy))
                .task(
                    "test",
                    command("cargo", CommandArgs::Static(vec!["test".to_owned()])),
                )
                .operation(
                    "countFiles",
                    OperationDefinition::new(|input, _services| {
                        let count = input
                            .get("files")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        Ok(serde_json::json!({ "count": count }))
                    }),
                )
                .binding(
                    bind(SurfaceTrigger {
                        surface: "cli".to_owned(),
                        name: "count".to_owned(),
                        phase: None,
                    })
                    .to("countFiles")
                    .finish(),
                );
        });

        assert!(definition.tools.contains_key("cargo"));
        assert!(definition.policies.contains_key("allow"));
        assert!(definition.tasks.contains_key("test"));
        assert!(definition.operations.contains_key("countFiles"));
        assert_eq!(definition.bindings[0].operation_name, "countFiles");
    }
}
