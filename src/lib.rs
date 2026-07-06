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
//! | [`surfaces`] | Surface triggers; agent-hook runtime and config generation |
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

pub use bindings::Binding;
pub use cli::{
    CompileRunweaverBinaryRequest, CompileRunweaverBinaryResult, CompiledRunweaverProject,
    CompiledRunweaverProjectBuilder, RunweaverCliIo, RunweaverStdin, compiled_runweaver_project,
    run_compiled_runweaver_project_cli, run_compiled_runweaver_project_cli_with_compile,
    run_generic_runweaver_cli,
};
pub use config::{
    BuiltinRegistry, ExecutionContext, LoadedRunweaverManifest, ManifestLoadError, RunweaverConfig,
    RunweaverDefinition, RunweaverDefinitionManifest, default_builtin_registry,
    define_runweaver_with, load_runweaver_manifest, project,
};
pub use core::OperationDefinition;
pub use embedded::{
    CompileCargoRunweaverBinaryError, CompileCargoRunweaverBinaryOptions,
    CompileCargoRunweaverBinaryResult, compile_cargo_runweaver_binary,
};
pub use profiles::Profile;
pub use services::RunweaverServices;
pub use surfaces::SurfaceTrigger;
pub use surfaces::agent_hooks::{
    AgentsSurfaceDefaults, Harness, HarnessCodec, HarnessDefinition, HarnessHookConfig,
    HarnessHookConfigError, HarnessHookConfigRegistry, HarnessHookConfigRenderInput,
    HarnessHookConfigSet, HarnessHookGroup, HarnessOptions, HarnessTargetInput, HookBindingInput,
    HookBindingValidationInput, HookConfigCommand, HookEmission, HookEnv, HookEvent, HookOutcome,
    HookRequest, HookStage, RunweaverHookCommandCwd, define_harness, define_harness_hook_config,
    guard_destructive_command, hook_failure_reason, optional_bool, optional_string,
    outcome_to_emission, parse_payload, render_harness_hook_config_files, require_event_name,
    require_object, require_present_field, require_string, touched_path_candidates,
};
