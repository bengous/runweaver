//! Runweaver is a library for defining project automation — tools, policies,
//! task graphs, operations, and bindings — and wiring it into the surfaces
//! where automation actually runs: agent-harness hooks (Claude, Codex), Git
//! hooks, and CI workflows.
//!
//! A project describes *what* it automates once, in Rust, and Runweaver takes
//! care of executing tasks deterministically, routing surface events to
//! operations, and generating the native configuration files each surface
//! expects.
//!
//! # Core concepts
//!
//! - **Definition** — a [`RunweaverDefinition`] is the root aggregate: named
//!   tools, policies, tasks, operations, and bindings. It is authored with
//!   [`define_runweaver_with`], the [`project`] builder, or loaded from a
//!   manifest.
//! - **Config** — a [`RunweaverConfig`] is the derived task-runner view of a
//!   definition (tools + tasks + policies), consumed by the [`runtime`].
//! - **Manifest** — a [`RunweaverDefinitionManifest`] is the serializable,
//!   closure-free form of a definition. Executable behavior is referenced by
//!   builtin name and resolved against a [`BuiltinRegistry`] at load time via
//!   [`load_runweaver_manifest`]; missing builtins fail fast.
//! - **Tasks** — units of project work: command tasks invoke tools, action
//!   tasks run Rust closures, and series/parallel tasks compose other tasks.
//!   Policies gate task execution with allow/skip/deny verdicts.
//! - **Operations** — JSON-in/JSON-out functions ([`OperationDefinition`])
//!   that receive injected [`RunweaverServices`]. Operations are the unit of
//!   work that surfaces trigger.
//! - **Bindings** — a [`Binding`] routes a [`SurfaceTrigger`] (for example a
//!   Claude pre-tool hook) to a named operation, optionally wrapped in
//!   profiles.
//! - **Surfaces** — the places automation runs. The [`surfaces::agent_hooks`]
//!   module owns the agent-hook surface end to end: harness codecs, hook
//!   dispatch, and generated native hook config files.
//! - **Profiles** — [`Profile`] middleware around operation execution
//!   (before/after/on-error hooks), used for cross-cutting gates such as
//!   stop-session validation.
//! - **Service ports** — [`RunweaverServices`] bundles the capability traits
//!   (filesystem, Git, process runner, session state, logger, env, clock,
//!   temp) that operations and profiles use instead of touching the host
//!   directly, keeping them testable.
//!
//! # Module map
//!
//! | Module | Role |
//! |---|---|
//! | [`config`] | Authoring: definitions, tasks, tools, policies, manifests |
//! | [`core`] | The operation primitive |
//! | [`runtime`] | Task and operation execution |
//! | [`bindings`] | Surface-trigger to operation routing |
//! | [`surfaces`] | Surface contracts; agent-hook runtime and config generation |
//! | [`profiles`] | Operation middleware and shipped validation profiles |
//! | [`services`] | Injected capability ports |
//! | [`diagnostics`] | Structured validation diagnostics |
//! | [`cli`] | The `runweaver` CLI and compiled project composition |
//! | [`embedded`] | Project-compiled binaries: embedded CLI, fingerprinting |
//! | [`toolchain`] | The managed `.runweaver/` toolchain directory |
//! | [`prelude`] | One-stop imports for project authors |
//!
//! # Getting started
//!
//! Project authors typically import from the [`prelude`], assemble a
//! definition with [`project`] or [`define_runweaver_with`], compose it into a
//! [`CompiledRunweaverProject`] with [`compiled_runweaver_project`], and expose
//! it through a binary that calls [`run_compiled_runweaver_project_cli`].

pub mod bindings;
pub mod cli;
pub mod config;
pub mod core;
pub mod diagnostics;
pub mod embedded;
pub mod prelude;
pub mod profiles;
pub mod runtime;
pub mod services;
pub mod surfaces;
pub mod toolchain;

pub use bindings::{
    Binding, BindingRegistry, BindingResolution, BindingRunError, BindingValidationIssue,
    BindingValidationResult, BoundOperationFn, BoundOperationRunResult, bind, resolve_binding,
    resolve_binding_trigger, run_bound_operation, run_resolved_binding, validate_binding_registry,
};
pub use cli::{
    CompileRunweaverBinaryRequest, CompileRunweaverBinaryResult, CompiledRunweaverProject,
    CompiledRunweaverProjectBuilder, LoadRunweaverAgentHooksConfigRequest,
    LoadRunweaverConfigRequest, RunweaverCliIo, RunweaverCliRuntime, RunweaverJsonMode,
    RunweaverParsedOptions, RunweaverStdin, compiled_runweaver_project, parse_runweaver_options,
    run_compiled_runweaver_cli, run_compiled_runweaver_cli_with_compile,
    run_compiled_runweaver_project_cli, run_compiled_runweaver_project_cli_with_compile,
    run_runweaver_cli, runweaver_help_text,
};

pub use config::{
    ActionFn, ActionOptions, ActionResult, ActionTask, ActionTaskOptions, AgentHooksConfigBuilder,
    AgentHooksConfigHookManifest, AgentHooksConfigManifest, AgentsBuiltinGuardManifest,
    AgentsPipelineSlotManifest, AgentsPreToolGuardManifest, AgentsSurfaceManifest, BindingManifest,
    BindingsBuilder, BuiltinRegistry, CiSurfaceManifest, CommandArgs, CommandArgsFn,
    CommandOptions, CommandTask, CommandTaskOptions, CompletedActionResultBuilder,
    CompositeOptions, CompositeTaskOptions, EmptyScope, EmptyScopeManifest, ExecutionContext,
    ExitCodeRule, FileTargetPolicyOptions, FileTargets, FileTargetsManifest, FileTargetsOptions,
    GeneratedSurfaceFile, GitFilesScopeManifest, GitPipelineSlotManifest, GitPreCommitSlotManifest,
    GitSurfaceManifest, GitToolSlotManifest, GithubCiSurfaceManifest, HarnessRef,
    HarnessTargetManifest, HookAppBuilder, HookBindingManifest, HookCommandBuilder,
    HookCommandManifest, HookCommandRef, HostCommandDefinition, LoadedRunweaverManifest,
    ManagedToolDefinition, ManifestLoadError, NamedDiagnosticsParserManifest, NextExecutionContext,
    OperationRef, OperationsBuilder, ParallelTask, PipelineDefinitionManifest, PoliciesBuilder,
    PolicyDefinition, PolicyFn, PolicyRef, PolicyVerdict, ProfileManifest, ProjectBuildError,
    ProjectBuilder, ProjectRunweaver, RUNWEAVER_DEFINITION_MANIFEST_VERSION, ResultMapping,
    RunweaverConfig, RunweaverConfigBuilder, RunweaverDefinition, RunweaverDefinitionBuilder,
    RunweaverDefinitionManifest, RunweaverDefinitionValidation, RunweaverOperationDefinition,
    RunweaverOperationDefinitionManifest, RunweaverOperationRegistry, SeriesTask, SurfacesManifest,
    TaskCompletion, TaskDefinition, TaskKind, TaskOutput, TaskPolicies, TaskRef, TaskRun,
    TaskRunStatus, ToolConfig, ToolDefinition, ToolDefinitionManifest, ToolRef,
    ToolTargetsManifest, ToolsBuilder, action, action_with, action_with_options,
    agent_hooks_config, allow, command, command_with, command_with_options,
    create_runweaver_definition_manifest, default_builtin_registry, define_config,
    define_config_with, define_runweaver, define_runweaver_with, deny, file_target_policy,
    file_target_verdict, file_targets, harness_ref, hook_app, hook_command, hook_command_ref,
    host_command, is_blocking_completion, load_runweaver_definition, load_runweaver_manifest,
    normalize_file_path, operation_ref, parallel, parallel_with, parallel_with_options, policy,
    policy_ref, project, runweaver_manifest_json_schema, runweaver_manifest_schema_content,
    runweaver_manifest_schema_sha256, series, series_with, series_with_options, skip,
    skip_with_reason, task_ref, tool, tool_ref, validate_config, validate_project,
    validate_runweaver_definition, write_runweaver_manifest, write_runweaver_manifest_schema,
};
pub use core::{OperationDefinition, OperationError, OperationExecuteFn, define_operation};
pub use diagnostics::{
    RunweaverDiagnostic, RunweaverDiagnosticSeverity, RunweaverDiagnosticsError, diagnostic,
    error_diagnostic, format_diagnostic, format_diagnostics, has_error_diagnostics,
    warning_diagnostic,
};
pub use embedded::{
    CompileCargoRunweaverBinaryError, CompileCargoRunweaverBinaryOptions,
    CompileCargoRunweaverBinaryResult, EmbeddedRunweaverCliIo, EmbeddedRunweaverJsonMode,
    EmbeddedRunweaverParsedOptions, EmbeddedRunweaverRuntime, EmbeddedRunweaverStdin,
    RUNWEAVER_BINARY_MANIFEST_VERSION, RunweaverBinaryManifest, RunweaverBinaryManifestInput,
    RunweaverFingerprintError, compile_cargo_runweaver_binary, create_runweaver_binary_manifest,
    embedded_runweaver_help_text, fingerprint_manifest_inputs, parse_embedded_runweaver_options,
    read_runweaver_binary_manifest_inputs, run_embedded_runweaver_cli,
};
pub use profiles::{
    AfterOperationFn, AgentPostEditFeedbackCheckResult, AgentPostEditFeedbackInput,
    AgentPostEditFeedbackPorts, AgentPostEditFeedbackProfileOptions, AgentPostEditFeedbackResult,
    AgentPostEditUpdatedFile, BeforeOperationFn, GeneratedFileGuard, GeneratedFileGuardFileRule,
    GeneratedFileGuardOptions, GeneratedFileGuardPatternRule, GeneratedFileGuardPredicate,
    GeneratedFileGuardPredicateRule, GeneratedFileGuardPrefixRule, GeneratedFileGuardResult,
    OnOperationErrorFn, Profile, ProfileError, StopSessionFingerprint,
    StopSessionFingerprintResult, StopSessionGeneratedGuardInput, StopSessionGeneratedGuardResult,
    StopSessionValidationBlockedError, StopSessionValidationEnv, StopSessionValidationEnvFn,
    StopSessionValidationEnvInput, StopSessionValidationInput, StopSessionValidationOptions,
    StopSessionValidationPorts, StopSessionValidationResult, StopSessionValidationRunInput,
    StopSessionValidationRunResult, agent_post_edit_feedback_profile,
    create_stop_session_validation_profile, define_profile, generated_file_guard,
    run_agent_post_edit_feedback, run_stop_session_validation,
};
pub use runtime::{
    CompactTaskOutput, CompactTaskRun, CreateExecutionContextOptions, RunweaverOperationRunError,
    RunweaverOperationRunResult, aggregate_task_completion, aggregate_task_output,
    compact_run_for_agents, create_execution_context, format_notable_runs, is_blocking_run,
    map_task_completion, normalize_files, run_bound_runweaver_operation,
    run_resolved_runweaver_binding, run_runweaver_operation, run_runweaver_operation_as_json,
    run_task, task_run_result_label,
};
pub use services::{
    ChangedFilesOptions, ClockPort, EnvPort, FileSystemPort, GitPort, LogFields, LoggerPort,
    ProcessRunOptions, ProcessRunOutput, ProcessRunnerPort, RunweaverServices, ServicePortError,
    SessionStatePort, TempDirectoryOptions, TempFileOptions, TempPort,
};
pub use surfaces::{
    SurfaceCodec, SurfaceDecodeFn, SurfaceDefinition, SurfaceEncodeFn, SurfaceEvent,
    SurfaceResponse, SurfaceResponseStatus, SurfaceTrigger, define_surface,
};

pub use surfaces::agent_hooks::{
    AgentHooksApp, AgentHooksAppDefinition, AgentHooksAppError, AgentHooksCliOptions,
    AgentHooksCliRequest, AgentHooksConfig, AgentHooksConfigCliError, AgentHooksConfigCliOptions,
    AgentHooksConfigCommand, AgentHooksConfigDefinition, AgentHooksConfigError,
    AgentHooksConfigHook, AgentHooksProcessIo, AgentHooksStdin, BuiltInHarnessName, ClaudeCodec,
    CodexCodec, CompiledRunweaverHookCommandOptions, DESTRUCTIVE_COMMAND_MERGE_HINT, Harness,
    HarnessCodec, HarnessDefinition, HarnessError, HarnessHookCommand, HarnessHookConfig,
    HarnessHookConfigCheckResult, HarnessHookConfigError, HarnessHookConfigMismatch,
    HarnessHookConfigRegistry, HarnessHookConfigRenderFn, HarnessHookConfigRenderInput,
    HarnessHookConfigSet, HarnessHookFile, HarnessHookGroup, HarnessOptions, HarnessRegistry,
    HarnessTarget, HarnessTargetInput, HarnessTargetValidationFn, HookApp, HookBinding,
    HookBindingInput, HookBindingValidationFn, HookBindingValidationInput, HookCommandCatalog,
    HookCommandError, HookCommandInput, HookCommandPrefixError, HookCommandSpec, HookConfigCommand,
    HookEmission, HookEnv, HookError, HookEvent, HookEventField, HookEventFieldValue,
    HookFeedbackOutcome, HookFn, HookInputError, HookOutcome, HookProcedure, HookRequest,
    HookStage, HookStopSessionInput, HookTouchedPathsInput, LoadAgentHooksConfigRequest,
    ParsedAgentHooksConfigArgs, ProjectedHookCommandOptions, RunHookInput, RuntimeServicesFactory,
    RuntimeServicesRunner, RunweaverHookCommandCwd, RunweaverHookCommandOptions, TaskHook,
    TaskHookCwdFn, TaskHookFilesFn, TaskHookFormatFailureFn, TaskHookRunner, TaskHookRunnerOptions,
    TaskHookRunnerSource, UpdatedFileSnapshot, agent_hooks_config_usage, block_reason_outcome,
    check_destructive_command, check_destructive_merge, check_harness_hook_config_files,
    claude_harness, claude_harness_hook_config, codex_harness, codex_harness_hook_config,
    compiled_runweaver_hook_command, create_hook_command_catalog, define_agent_hooks_app,
    define_agent_hooks_config, define_harness, define_harness_hook_config, define_hook,
    define_hook_command, define_task_hook, execute_hook_command, feedback_outcome,
    format_task_run_as_hook_block_reason, guard_destructive, guard_destructive_command,
    harness_hook_config_registry_from_harnesses, harness_registry_from_harnesses,
    hook_command_input, hook_failure_reason, hook_feedback_from_task_run, hook_groups_by_stage,
    hook_stop_session_input, hook_tool_touched_paths_input, is_hook_feedback_outcome,
    optional_bool, optional_string, outcome_to_emission, parse_agent_hooks_config_args,
    parse_payload, render_harness_hook_config_files, require_event_name, require_hook_event_field,
    require_object, require_present_field, require_string, run_agent_hooks_config_process_main,
    run_agent_hooks_main, run_agent_hooks_process_main, run_hook, run_hook_args, run_hook_command,
    run_hook_command_with_env, runweaver_hook_command, task_hook_runner, touched_path_candidates,
    validate_harness_hook_config_set, with_runtime_services, write_harness_hook_config_files,
};
