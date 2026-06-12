//! Authoring surface for Runweaver definitions: tools, policies, tasks,
//! operations, bindings, and their serializable manifest form.
//!
//! There are three ways to produce a definition, all converging on
//! [`RunweaverDefinition`]:
//!
//! - **Builder closures** — [`define_runweaver_with`] (full definition) or
//!   [`define_config_with`] (task config only) for direct, imperative
//!   composition.
//! - **Declarative project builder** — [`project`] returns a
//!   [`ProjectBuilder`] using typed refs ([`ToolRef`], [`TaskRef`],
//!   [`PolicyRef`], [`OperationRef`]) so cross-references are checked at the
//!   build boundary.
//! - **Manifest loading** — [`load_runweaver_manifest`] turns a pure-data
//!   [`RunweaverDefinitionManifest`] plus a [`BuiltinRegistry`] (which supplies
//!   the executable closures the manifest references by name) into a
//!   [`LoadedRunweaverManifest`]: the definition, optional agent-hook config,
//!   and generated surface files.
//!
//! Tasks come in four kinds ([`TaskDefinition`]): command tasks invoke a
//! named tool, action tasks run a Rust closure, and series/parallel tasks
//! compose other tasks by name with optional fail-fast behavior. Policies
//! ([`PolicyDefinition`]) gate execution with a [`PolicyVerdict`]
//! (allow/skip/deny); [`file_target_policy`] builds the common
//! "only run for matching files" gate from [`FileTargets`].
//!
//! [`validate_runweaver_definition`] checks a definition for dangling
//! references, task cycles, and bindings that point at missing operations,
//! returning [`RunweaverDiagnostic`](crate::diagnostics::RunweaverDiagnostic)
//! values rather than panicking.
//!
//! The derived [`RunweaverConfig`] view is what the [`runtime`](crate::runtime)
//! executes; bindings and operations are executed through
//! [`crate::runtime`] and [`crate::bindings`].

pub mod declarative;
pub(crate) mod file_targets;
pub(crate) mod manifest;
pub(crate) mod runweaver;
pub(crate) mod tasks;
pub(crate) mod validate;

pub use declarative::{
    ActionOptions, ActionTaskBuilder, AgentHooksConfigBuilder, BindingsBuilder, CommandOptions,
    CommandTaskBuilder, CompositeOptions, CompositeTaskBuilder, HarnessRef, HookAppBuilder,
    HookCommandBuilder, HookCommandRef, ManagedToolBuilder, OperationRef, OperationsBuilder,
    PoliciesBuilder, PolicyRef, ProjectBuildError, ProjectBuilder, ProjectRunweaver, TaskRef,
    TasksBuilder, ToolRef, ToolsBuilder, action_with, agent_hooks_config, command_with,
    error_codes, error_otherwise, harness_ref, hook_app, hook_command, hook_command_ref,
    operation_ref, parallel_with, policy_ref, project, result_mapping, series_with, success_codes,
    task_ref, tool_error_codes, tool_error_otherwise, tool_ref, warning_codes,
};
pub use file_targets::{
    EmptyScope, FileTargetPolicyOptions, FileTargets, FileTargetsOptions, file_target_policy,
    file_target_verdict, file_targets, normalize_file_path,
};
pub use manifest::{
    AgentHooksConfigHookManifest, AgentHooksConfigManifest, AgentsBuiltinGuardManifest,
    AgentsPipelineSlotManifest, AgentsPreToolGuardManifest, AgentsSurfaceManifest, BindingManifest,
    BuiltinRegistry, CiSurfaceManifest, EmptyScopeManifest, FileTargetsManifest,
    GeneratedSurfaceFile, GitFilesScopeManifest, GitPipelineSlotManifest, GitPreCommitSlotManifest,
    GitSurfaceManifest, GitToolSlotManifest, GithubCiSurfaceManifest, HarnessTargetManifest,
    HookBindingManifest, HookCommandManifest, LoadedRunweaverManifest, ManifestLoadError,
    NamedDiagnosticsParserManifest, PipelineDefinitionManifest, ProfileManifest,
    RUNWEAVER_DEFINITION_MANIFEST_VERSION, RunweaverDefinitionManifest,
    RunweaverOperationDefinitionManifest, RunweaverProjectBinary, SurfacesManifest,
    ToolDefinitionManifest, ToolTargetsManifest, create_runweaver_definition_manifest,
    load_runweaver_definition, load_runweaver_manifest, runweaver_manifest_json_schema,
    runweaver_manifest_schema_content, runweaver_manifest_schema_sha256, write_runweaver_manifest,
    write_runweaver_manifest_schema,
};
pub use runweaver::{
    RunweaverDefinition, RunweaverDefinitionBuilder, RunweaverOperationDefinition,
    RunweaverOperationRegistry, define_runweaver, define_runweaver_with,
};
pub use tasks::{
    ActionFn, ActionResult, ActionTask, ActionTaskOptions, CommandArgs, CommandArgsFn, CommandTask,
    CommandTaskOptions, CompletedActionResultBuilder, CompositeTaskOptions, ExecutionContext,
    ExitCodeRule, HostCommandDefinition, ManagedToolDefinition, NextExecutionContext, ParallelTask,
    PolicyDefinition, PolicyFn, PolicyVerdict, ResultMapping, RunweaverConfig,
    RunweaverConfigBuilder, SeriesTask, TaskCompletion, TaskDefinition, TaskKind, TaskOutput,
    TaskPolicies, TaskRun, TaskRunStatus, ToolConfig, ToolDefinition, action, action_with_options,
    allow, command, command_with_options, define_config, define_config_with, deny, host_command,
    is_blocking_completion, parallel, parallel_with_options, policy, series, series_with_options,
    skip, skip_with_reason, tool,
};
pub use validate::{
    RunweaverDefinitionValidation, validate_config, validate_project, validate_runweaver_definition,
};
