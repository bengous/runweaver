//! The agent-hook surface: dispatching harness hook events to Rust hook
//! commands and keeping each harness's native hook configuration generated
//! and in sync.
//!
//! # Hook lifecycle
//!
//! An agent harness (Claude, Codex, or a custom one) invokes a hook process
//! at a [`HookStage`] — pre-tool, post-edit, or stop — passing a native JSON
//! payload on stdin. A [`HarnessCodec`] decodes that payload into a
//! normalized [`HookEvent`]; a [`HookCommandSpec`] (a named Rust closure,
//! registered per stage) inspects the event and returns a [`HookOutcome`]:
//! pass, or block with a reason, optionally carrying a system message and an
//! [`UpdatedFileSnapshot`] for hooks that rewrite file content. The codec
//! then encodes the outcome back into the harness's native response shape as
//! a [`HookEmission`] (exit code + stdout/stderr). Failures at any step are
//! encoded with the codec's failure path rather than crashing the hook
//! process.
//!
//! [`run_hook`](run_hook()) runs that pipeline once; [`run_agent_hooks_main`] and
//! [`run_agent_hooks_process_main`] are the process entry points that
//! dispatch `hook <harness> <command>` invocations against a [`HookApp`] /
//! [`AgentHooksApp`].
//!
//! # Harnesses and generated config
//!
//! A [`Harness`] couples a codec with a [`HarnessHookConfig`] that knows how
//! to render the harness's native hook configuration file (for the built-ins:
//! `.claude/settings.json` via [`claude_harness`], `.codex/config.toml` via
//! [`codex_harness`]). [`render_harness_hook_config_files`],
//! [`check_harness_hook_config_files`], and
//! [`write_harness_hook_config_files`] implement the render/check/sync cycle
//! so generated configs never drift from the Rust definition. The command
//! line each harness invokes is produced by [`runweaver_hook_command`] or, for
//! project-compiled binaries, [`compiled_runweaver_hook_command`].
//!
//! # Composition
//!
//! [`define_agent_hooks_config`] assembles the whole surface — harness
//! registry, hook commands, bindings ([`HookBinding`]) and target config
//! files ([`HarnessTarget`]) — into an [`AgentHooksConfig`], validating that
//! every binding references a registered command and harness. Task-backed
//! hooks bridge through [`TaskHook`]/[`task_hook_runner`], which run a
//! Runweaver task and convert its [`TaskRun`](crate::config::TaskRun) into
//! hook feedback ([`hook_feedback_from_task_run`]). The [`agents_surface`]
//! pipeline owns touched-path tracking and the generic post-edit/stop
//! validation flows.
//!
//! # Semantics worth knowing
//!
//! - [`HookOutcome`] is the command's decision; [`HookEmission`] is the
//!   harness-native projection of it.
//! - Pre-tool blocks emit exit code 2 (harnesses treat it as a hard deny);
//!   post-edit and stop blocks emit exit code 1.
//! - A post-edit outcome's `updated_file` lets the harness replace the tool
//!   output with rewritten content (Claude's `updatedToolOutput`).
//! - `block_reason` is the agent-facing rationale; `system_message` is
//!   additional context, not a block.

pub mod agent_hooks_config;
pub mod agent_hooks_config_cli;
pub mod agent_hooks_main;
pub mod agents_surface;
pub mod app;
pub mod built_in_harnesses;
pub mod codecs;
pub mod command_prefix;
pub mod contract;
pub mod destructive_commands;
pub mod failure;
pub mod harness;
pub mod harness_hook_config;
pub mod hook_command;
pub mod inputs;
pub mod payload;
pub mod run_hook;
pub mod runtime;
pub mod task_hook;
pub mod tool_input;

pub use agent_hooks_config::{
    AgentHooksConfig, AgentHooksConfigDefinition, AgentHooksConfigError, AgentHooksConfigHook,
    define_agent_hooks_config, define_hook,
};
pub use agent_hooks_config_cli::{
    AgentHooksConfigCliError, AgentHooksConfigCliOptions, AgentHooksConfigCommand,
    LoadAgentHooksConfigRequest, ParsedAgentHooksConfigArgs, agent_hooks_config_usage,
    parse_agent_hooks_config_args, run_agent_hooks_config_process_main,
};
pub use agent_hooks_main::{
    AgentHooksCliOptions, AgentHooksCliRequest, AgentHooksProcessIo, AgentHooksStdin,
    execute_hook_command, run_agent_hooks_main, run_agent_hooks_process_main,
};
pub use agents_surface::{
    AgentsPipelineStage, ChangedFileSnapshot, PipelineOutcome, run_pipeline_with_outcome,
    run_post_edit_pipeline, run_stop_pipeline,
};
pub use app::{AgentHooksApp, AgentHooksAppDefinition, AgentHooksAppError, define_agent_hooks_app};
pub use built_in_harnesses::{BuiltInHarnessName, claude_harness, codex_harness};
pub use codecs::{ClaudeCodec, CodexCodec};
pub use command_prefix::{
    CompiledRunweaverHookCommandOptions, HookCommandPrefixError, RunweaverHookCommandCwd,
    RunweaverHookCommandOptions, compiled_runweaver_hook_command, runweaver_hook_command,
};
pub use contract::{
    HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage, UpdatedFileSnapshot,
};
pub use destructive_commands::{
    DESTRUCTIVE_COMMAND_MERGE_HINT, check_destructive_command, check_destructive_merge,
    guard_destructive, guard_destructive_command,
};
pub use failure::hook_failure_reason;
pub use harness::{
    AgentsSurfaceDefaults, Harness, HarnessDefinition, HarnessError, HarnessRegistry,
    HarnessTargetInput, HookBindingInput, define_harness,
    harness_hook_config_registry_from_harnesses, harness_registry_from_harnesses,
};
pub use harness_hook_config::{
    HarnessHookCommand, HarnessHookConfig, HarnessHookConfigCheckResult, HarnessHookConfigError,
    HarnessHookConfigMismatch, HarnessHookConfigRegistry, HarnessHookConfigRenderFn,
    HarnessHookConfigRenderInput, HarnessHookConfigSet, HarnessHookFile, HarnessHookGroup,
    HarnessOptions, HarnessTarget, HarnessTargetValidationFn, HookBinding, HookBindingValidationFn,
    HookBindingValidationInput, HookConfigCommand, check_harness_hook_config_files,
    claude_harness_hook_config, codex_harness_hook_config, define_harness_hook_config,
    hook_groups_by_stage, render_harness_hook_config_files, validate_harness_hook_config_set,
    write_harness_hook_config_files,
};
pub use hook_command::{
    HookCommandCatalog, HookCommandError, HookCommandSpec, HookEventField, HookEventFieldValue,
    HookFeedbackOutcome, HookFn, HookProcedure, ProjectedHookCommandOptions,
    RuntimeServicesFactory, RuntimeServicesRunner, block_reason_outcome,
    create_hook_command_catalog, define_hook_command, feedback_outcome, is_hook_feedback_outcome,
    require_hook_event_field, with_runtime_services,
};
pub use inputs::{
    HookCommandInput, HookInputError, HookStopSessionInput, HookTouchedPathsInput,
    hook_command_input, hook_stop_session_input, hook_tool_touched_paths_input,
};
pub use payload::{
    HookError, optional_bool, optional_string, parse_payload, require_event_name, require_object,
    require_present_field, require_string,
};
pub use run_hook::{RunHookInput, run_hook};
pub use runtime::{
    HarnessCodec, HookApp, outcome_to_emission, run_hook_args, run_hook_command,
    run_hook_command_with_env,
};
pub use task_hook::{
    DefineTaskHookOptions, TaskHook, TaskHookCwdFn, TaskHookFilesFn, TaskHookFormatFailureFn,
    TaskHookRunner, TaskHookRunnerOptions, TaskHookRunnerSource, define_task_hook,
    format_task_run_as_hook_block_reason, hook_feedback_from_task_run, task_hook_runner,
};
pub use tool_input::touched_path_candidates;
