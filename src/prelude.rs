//! One-stop imports for project authors.
//!
//! Re-exports the items needed to author a definition (builders, task and
//! policy helpers, typed refs), load or write manifests, compose a
//! [`CompiledRunweaverProject`], execute operations and bindings, and handle
//! the core agent-hook types. `use runweaver::prelude::*;` covers typical
//! project configuration code; reach into specific modules only for deeper
//! integration work.

pub use crate::cli::{
    CompiledRunweaverProject, CompiledRunweaverProjectBuilder, compiled_runweaver_project,
};
pub use crate::config::declarative::{
    ActionOptions, ActionTaskBuilder, AgentHooksConfigBuilder, BindingsBuilder, CommandOptions,
    CommandTaskBuilder, CompositeOptions, CompositeTaskBuilder, HarnessRef, HookAppBuilder,
    HookCommandBuilder, HookCommandRef, ManagedToolBuilder, OperationRef, OperationsBuilder,
    PoliciesBuilder, PolicyRef, ProjectBuildError, ProjectBuilder, ProjectRunweaver, TaskRef,
    TasksBuilder, ToolRef, ToolsBuilder, action, action_with, agent_hooks_config, command,
    command_with, hook_app, hook_command, operation_ref, parallel, parallel_with, result_mapping,
    series, series_with,
};
pub use crate::config::{
    ActionFn, ActionResult, AgentHooksConfigHookManifest, AgentHooksConfigManifest,
    AgentsBuiltinGuardManifest, AgentsPipelineSlotManifest, AgentsPreToolGuardManifest,
    AgentsSurfaceManifest, BindingManifest, BuiltinRegistry, CommandArgs, CommandArgsFn,
    CompletedActionResultBuilder, EmptyScope, EmptyScopeManifest, ExecutionContext, ExitCodeRule,
    FileTargetPolicyOptions, FileTargets, FileTargetsManifest, FileTargetsOptions,
    HarnessTargetManifest, HookBindingManifest, HookCommandManifest, LoadedRunweaverManifest,
    ManifestLoadError, NamedDiagnosticsParserManifest, NextExecutionContext,
    PipelineDefinitionManifest, PolicyFn, PolicyVerdict, ProfileManifest,
    RUNWEAVER_DEFINITION_MANIFEST_VERSION, ResultMapping, RunweaverConfig, RunweaverConfigBuilder,
    RunweaverDefinition, RunweaverDefinitionBuilder, RunweaverDefinitionManifest,
    RunweaverDefinitionValidation, RunweaverOperationDefinitionManifest, RunweaverProjectBinary,
    SurfacesManifest, TaskCompletion, TaskDefinition, TaskOutput, ToolConfig,
    ToolDefinitionManifest, ToolTargetsManifest, allow, create_runweaver_definition_manifest,
    define_config_with, define_runweaver_with, deny, error_codes, error_otherwise,
    file_target_policy, file_targets, harness_ref, hook_command_ref, load_runweaver_definition,
    load_runweaver_manifest, policy_ref, project, runweaver_manifest_json_schema, skip,
    skip_with_reason, success_codes, task_ref, tool_error_codes, tool_error_otherwise, tool_ref,
    validate_runweaver_definition, warning_codes, write_runweaver_manifest,
    write_runweaver_manifest_schema,
};
pub use crate::runtime::{
    RunweaverOperationRunError, RunweaverOperationRunResult, run_bound_runweaver_operation,
    run_resolved_runweaver_binding, run_runweaver_operation, run_runweaver_operation_as_json,
};
pub use crate::surfaces::agent_hooks::{HookApp, HookEvent, HookOutcome, HookStage};
