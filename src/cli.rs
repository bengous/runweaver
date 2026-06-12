//! The `runweaver` command-line interface and compiled-project composition.
//!
//! The CLI fronts a project's automation: `run <task>`, `check`,
//! `check hooks`, `check manifest`, `sync hooks`, `sync manifest`,
//! `hook <harness> <command>`,
//! `git-hook <slot>`, `init`, `install`, and `compile binary`. How
//! configuration is sourced is abstracted behind [`RunweaverCliRuntime`]
//! (loader/compiler ports) and all I/O behind [`RunweaverCliIo`], so the same
//! dispatcher serves tests, the generic `runweaver` binary, and
//! project-compiled binaries.
//!
//! [`CompiledRunweaverProject`] is the Rust-native project root: the full
//! [`RunweaverDefinition`], its derived task config, optional agent-hook
//! config, and generated surface files, assembled with
//! [`compiled_runweaver_project`] /
//! [`CompiledRunweaverProjectBuilder`]. Project binaries hand it to
//! [`run_compiled_runweaver_project_cli`] (or the `_with_compile` variant to
//! also support `compile binary`); [`run_runweaver_cli`] is the fully
//! port-driven entry point.
//!
//! Exit codes: `0` on success, `1` when any task run blocks or a diagnostic
//! error is reported. `--json` selects compact output (successful children
//! omitted — the agent-facing view) and `--json=full` the complete run tree.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};

use crate::bindings::{Binding, BindingResolution, BindingRunError, BoundOperationRunResult};
use crate::config::declarative::{
    AgentHooksConfigBuilder, agent_hooks_config as build_agent_hooks_config,
};
use crate::config::{
    GeneratedSurfaceFile, GitFilesScopeManifest, GitSurfaceManifest, RunweaverConfig,
    RunweaverDefinition, RunweaverDefinitionManifest, SurfacesManifest, TaskRun, TaskRunStatus,
};
use crate::diagnostics::{RunweaverDiagnosticsError, format_diagnostics, has_error_diagnostics};
use crate::embedded::RunweaverBinaryManifest;
use crate::runtime::{
    CreateExecutionContextOptions, RunweaverOperationRunError, RunweaverOperationRunResult,
    compact_run_for_agents, create_execution_context, format_notable_runs, is_blocking_run,
    run_bound_runweaver_operation, run_resolved_runweaver_binding, run_runweaver_operation,
    run_runweaver_operation_as_json, run_task, task_run_result_label,
};
use crate::services::RunweaverServices;
use crate::surfaces::agent_hooks::{
    AgentHooksConfig, AgentHooksConfigCliOptions, AgentHooksConfigCommand, AgentHooksConfigError,
    AgentHooksProcessIo, HookEnv, LoadAgentHooksConfigRequest as AgentHooksConfigCliLoadRequest,
    run_agent_hooks_config_process_main, run_agent_hooks_process_main,
};
use crate::toolchain::{
    ScaffoldActionStatus, install_managed_toolchain, scaffold_runweaver_project,
};

/// TypeScript declarations for [`RunweaverDefinitionManifest`], generated from
/// the manifest JSON schema and embedded so `manifest types` works without a
/// TypeScript toolchain. Regenerate with `scripts/generate-manifest-types.sh`.
const MANIFEST_TYPES_DTS: &str = include_str!("../assets/manifest.d.ts");

/// Loader and compiler ports the CLI dispatches through, decoupling command
/// handling from how a project sources its config.
pub struct RunweaverCliRuntime<'ports, 'config> {
    pub load_runweaver_config: &'ports dyn for<'request> Fn(
        LoadRunweaverConfigRequest<'request>,
    ) -> Result<RunweaverConfig>,
    pub load_agent_hooks_config: &'ports dyn for<'request> Fn(
        LoadRunweaverAgentHooksConfigRequest<'request>,
    )
        -> Result<AgentHooksConfig<'config>>,
    pub compile_binary: &'ports dyn for<'request> Fn(
        CompileRunweaverBinaryRequest<'request>,
    ) -> Result<CompileRunweaverBinaryResult>,
    pub generated_surface_files: &'ports dyn Fn() -> Result<Vec<GeneratedSurfaceFile>>,
    pub git_surface: &'ports dyn Fn() -> Result<Option<GitSurfaceManifest>>,
}

pub struct LoadRunweaverConfigRequest<'a> {
    pub cwd: &'a Path,
    pub config_path: Option<&'a str>,
}

pub struct LoadRunweaverAgentHooksConfigRequest<'a> {
    pub root: &'a Path,
    pub config_path: &'a str,
    pub export_name: Option<&'a str>,
}

pub struct CompileRunweaverBinaryRequest<'a> {
    pub cwd: &'a Path,
    pub config_path: Option<&'a str>,
    pub export_name: Option<&'a str>,
    pub out_path: &'a str,
    pub fingerprint_roots: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileRunweaverBinaryResult {
    pub outfile: PathBuf,
    pub manifest: RunweaverBinaryManifest,
}

/// A project's complete compiled automation root: the full definition, the
/// derived task config, optional agent-hook config, and generated surface
/// files. Built with [`compiled_runweaver_project`]; consumed by
/// [`run_compiled_runweaver_project_cli`].
#[derive(Debug, Clone)]
pub struct CompiledRunweaverProject<'config> {
    runweaver_definition: RunweaverDefinition,
    runweaver_config: RunweaverConfig,
    agent_hooks_config: Option<AgentHooksConfig<'config>>,
    generated_surface_files: Vec<GeneratedSurfaceFile>,
    surfaces: Option<SurfacesManifest>,
}

#[derive(Debug, Clone)]
pub struct CompiledRunweaverProjectBuilder<'config> {
    project: CompiledRunweaverProject<'config>,
}

/// Starts a [`CompiledRunweaverProjectBuilder`] from a definition; attach
/// hook config with `.agent_hooks_config(..)` or `.agent_hooks_config_with(..)`
/// and call `.build()`.
pub fn compiled_runweaver_project<'config>(
    runweaver_definition: impl Into<RunweaverDefinition>,
) -> CompiledRunweaverProjectBuilder<'config> {
    CompiledRunweaverProjectBuilder {
        project: CompiledRunweaverProject::new(runweaver_definition),
    }
}

impl<'config> CompiledRunweaverProjectBuilder<'config> {
    pub fn agent_hooks_config(mut self, agent_hooks_config: AgentHooksConfig<'config>) -> Self {
        self.project.agent_hooks_config = Some(agent_hooks_config);
        self
    }

    pub fn agent_hooks_config_with(
        mut self,
        name: impl Into<String>,
        binary_name: impl Into<String>,
        source_path: impl Into<String>,
        configure: impl FnOnce(&mut AgentHooksConfigBuilder<'config>),
    ) -> std::result::Result<Self, AgentHooksConfigError> {
        let mut builder = build_agent_hooks_config(name, binary_name, source_path);
        configure(&mut builder);
        self.project.agent_hooks_config = Some(builder.build()?);
        Ok(self)
    }

    pub fn build(self) -> CompiledRunweaverProject<'config> {
        self.project
    }
}

impl<'config> CompiledRunweaverProject<'config> {
    pub fn new(runweaver_definition: impl Into<RunweaverDefinition>) -> Self {
        let runweaver_definition = runweaver_definition.into();
        let runweaver_config = runweaver_definition.task_config();
        Self {
            runweaver_definition,
            runweaver_config,
            agent_hooks_config: None,
            generated_surface_files: Vec::new(),
            surfaces: None,
        }
    }

    pub fn with_agent_hooks_config(
        mut self,
        agent_hooks_config: AgentHooksConfig<'config>,
    ) -> Self {
        self.agent_hooks_config = Some(agent_hooks_config);
        self
    }

    pub fn with_generated_surface_files(mut self, files: Vec<GeneratedSurfaceFile>) -> Self {
        self.generated_surface_files = files;
        self
    }

    pub fn with_surfaces(mut self, surfaces: Option<SurfacesManifest>) -> Self {
        self.surfaces = surfaces;
        self
    }

    pub fn runweaver_config(&self) -> &RunweaverConfig {
        &self.runweaver_config
    }

    pub fn runweaver_definition(&self) -> &RunweaverDefinition {
        &self.runweaver_definition
    }

    pub fn agent_hooks_config(&self) -> Option<&AgentHooksConfig<'config>> {
        self.agent_hooks_config.as_ref()
    }

    pub fn generated_surface_files(&self) -> &[GeneratedSurfaceFile] {
        &self.generated_surface_files
    }

    pub fn git_surface(&self) -> Option<&GitSurfaceManifest> {
        self.surfaces
            .as_ref()
            .and_then(|surfaces| surfaces.git.as_ref())
    }

    pub fn run_operation(
        &self,
        operation_name: &str,
        input: serde_json::Value,
        execution_context: crate::config::ExecutionContext,
        services: &RunweaverServices<'_>,
    ) -> std::result::Result<RunweaverOperationRunResult, RunweaverOperationRunError> {
        run_runweaver_operation(
            &self.runweaver_definition,
            operation_name,
            input,
            execution_context,
            services,
        )
    }

    pub fn run_operation_as_json(
        &self,
        operation_name: &str,
        input: serde_json::Value,
        execution_context: crate::config::ExecutionContext,
        services: &RunweaverServices<'_>,
    ) -> std::result::Result<serde_json::Value, RunweaverOperationRunError> {
        run_runweaver_operation_as_json(
            &self.runweaver_definition,
            operation_name,
            input,
            execution_context,
            services,
        )
    }

    pub fn run_bound_operation(
        &self,
        binding: &Binding,
        input: serde_json::Value,
        binding_context: &mut serde_json::Value,
        execution_context: crate::config::ExecutionContext,
        services: &RunweaverServices<'_>,
    ) -> std::result::Result<serde_json::Value, BindingRunError> {
        run_bound_runweaver_operation(
            &self.runweaver_definition,
            binding,
            input,
            binding_context,
            execution_context,
            services,
        )
    }

    pub fn run_resolved_binding(
        &self,
        resolution: &BindingResolution,
        input: serde_json::Value,
        binding_context: &mut serde_json::Value,
        execution_context: crate::config::ExecutionContext,
        services: &RunweaverServices<'_>,
    ) -> std::result::Result<BoundOperationRunResult, BindingRunError> {
        run_resolved_runweaver_binding(
            &self.runweaver_definition,
            resolution,
            input,
            binding_context,
            execution_context,
            services,
        )
    }
}

pub enum RunweaverStdin<'io> {
    Text(&'io str),
    Reader(&'io mut dyn FnMut() -> Result<String>),
}

impl RunweaverStdin<'_> {
    fn read(&mut self) -> Result<String> {
        match self {
            Self::Text(stdin) => Ok((*stdin).to_owned()),
            Self::Reader(read_stdin) => read_stdin(),
        }
    }
}

/// All CLI I/O — stdin, stdout, stderr, and the process environment — so
/// callers (binaries, tests) own the streams.
pub struct RunweaverCliIo<'io> {
    pub stdin: RunweaverStdin<'io>,
    pub stdout: &'io mut dyn Write,
    pub stderr: &'io mut dyn Write,
    pub env: &'io HookEnv,
}

/// JSON output selection: `Off` for human text, `Compact` (`--json`) for
/// the agent-facing view with successful children omitted, `Full`
/// (`--json=full`) for the complete run tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunweaverJsonMode {
    Off,
    Compact,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunweaverParsedOptions {
    pub cwd: Option<String>,
    pub config_path: Option<String>,
    pub export_name: Option<String>,
    pub json: RunweaverJsonMode,
    pub verbose: bool,
    pub files: Vec<String>,
    pub input_json: Option<String>,
    pub out_path: Option<String>,
    pub fingerprint_roots: Vec<String>,
    pub positionals: Vec<String>,
}

/// Port-driven CLI entry point: dispatches `args` against the loaders and
/// compiler in `runtime`. Diagnostic errors are rendered to stderr and map
/// to exit code 1; other errors propagate.
pub fn run_runweaver_cli(
    args: &[String],
    runtime: RunweaverCliRuntime<'_, '_>,
    mut io: RunweaverCliIo<'_>,
) -> Result<i32> {
    match run_runweaver_cli_inner(args, runtime, &mut io) {
        Ok(exit_code) => Ok(exit_code),
        Err(error) => match error.downcast::<RunweaverDiagnosticsError>() {
            Ok(error) => {
                writeln!(io.stderr, "{}", format_diagnostics(&error.diagnostics))?;
                Ok(1)
            }
            Err(error) => Err(error),
        },
    }
}

/// Runs the CLI against already-built Rust configs instead of loading authored config files.
pub fn run_compiled_runweaver_cli<'config>(
    args: &[String],
    runweaver_config: &RunweaverConfig,
    agent_hooks_config: Option<&AgentHooksConfig<'config>>,
    io: RunweaverCliIo<'_>,
) -> Result<i32> {
    let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
        Err(anyhow!(
            "compile binary is not available from in-process compiled config; build a project-specific Rust binary instead."
        ))
    };

    run_compiled_runweaver_cli_with_compile(
        args,
        runweaver_config,
        agent_hooks_config,
        &compile,
        io,
    )
}

/// Runs the CLI against a compiled Rust project composition root.
pub fn run_compiled_runweaver_project_cli<'config>(
    args: &[String],
    project: &CompiledRunweaverProject<'config>,
    io: RunweaverCliIo<'_>,
) -> Result<i32> {
    let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
        Err(anyhow!(
            "compile binary is not available from in-process compiled config; build a project-specific Rust binary instead."
        ))
    };

    run_compiled_runweaver_project_cli_with_compile(args, project, &compile, io)
}

/// Runs the compiled project CLI with a project-specific binary compiler.
pub fn run_compiled_runweaver_project_cli_with_compile<'config>(
    args: &[String],
    project: &CompiledRunweaverProject<'config>,
    compile_binary: &dyn for<'request> Fn(
        CompileRunweaverBinaryRequest<'request>,
    ) -> Result<CompileRunweaverBinaryResult>,
    io: RunweaverCliIo<'_>,
) -> Result<i32> {
    let args = compiled_cli_args(args);
    let load_config =
        |_request: LoadRunweaverConfigRequest<'_>| Ok(project.runweaver_config().clone());
    let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| {
        project.agent_hooks_config().cloned().ok_or_else(|| {
            anyhow!("Compiled Runweaver CLI was called without compiled agent hook config.")
        })
    };
    let generated_surface_files = || Ok(project.generated_surface_files().to_vec());
    let git_surface = || Ok(project.git_surface().cloned());

    run_runweaver_cli(
        &args,
        RunweaverCliRuntime {
            load_runweaver_config: &load_config,
            load_agent_hooks_config: &load_hooks,
            compile_binary,
            generated_surface_files: &generated_surface_files,
            git_surface: &git_surface,
        },
        io,
    )
}

/// Runs the compiled CLI with a project-specific binary compiler for `compile binary`.
pub fn run_compiled_runweaver_cli_with_compile<'config>(
    args: &[String],
    runweaver_config: &RunweaverConfig,
    agent_hooks_config: Option<&AgentHooksConfig<'config>>,
    compile_binary: &dyn for<'request> Fn(
        CompileRunweaverBinaryRequest<'request>,
    ) -> Result<CompileRunweaverBinaryResult>,
    io: RunweaverCliIo<'_>,
) -> Result<i32> {
    let args = compiled_cli_args(args);
    let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(runweaver_config.clone());
    let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| {
        agent_hooks_config.cloned().ok_or_else(|| {
            anyhow!("Compiled Runweaver CLI was called without compiled agent hook config.")
        })
    };
    let generated_surface_files = || Ok(Vec::new());
    let git_surface = || Ok(None);

    run_runweaver_cli(
        &args,
        RunweaverCliRuntime {
            load_runweaver_config: &load_config,
            load_agent_hooks_config: &load_hooks,
            compile_binary,
            generated_surface_files: &generated_surface_files,
            git_surface: &git_surface,
        },
        io,
    )
}

/// Loads a project for the generic `runweaver` binary: reads
/// `.runweaver/manifest.json` under `root` and resolves it against the
/// library's [`default_builtin_registry`](crate::config::default_builtin_registry).
/// Manifests that reference builtins outside the default registry fail fast
/// with a pointer to project-specific binaries.
pub fn load_generic_runweaver_project(root: &Path) -> Result<CompiledRunweaverProject<'static>> {
    let manifest_path = root.join(".runweaver/manifest.json");
    let content = std::fs::read_to_string(&manifest_path).map_err(|error| {
        anyhow!(
            "Failed to read Runweaver manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest: RunweaverDefinitionManifest =
        serde_json::from_str(&content).map_err(|error| {
            anyhow!(
                "Failed to parse Runweaver manifest JSON {}: {error}",
                manifest_path.display()
            )
        })?;
    let loaded = crate::config::load_runweaver_manifest(
        &manifest,
        &crate::config::default_builtin_registry(),
        &crate::config::generic_runweaver_project_binary(),
    )
    .map_err(|error| match error {
        crate::config::ManifestLoadError::UnknownBuiltins(builtins) => anyhow!(
            "Runweaver manifest {} references builtins missing from the generic runweaver registry:\n{builtins}\nThis manifest requires a project-specific runweaver binary that registers those builtins.",
            manifest_path.display()
        ),
        other => anyhow!(other),
    })?;
    let mut builder = compiled_runweaver_project(loaded.definition);
    if let Some(agent_hooks) = loaded.agent_hooks {
        builder = builder.agent_hooks_config(agent_hooks);
    }
    Ok(builder
        .build()
        .with_generated_surface_files(loaded.generated_surface_files)
        .with_surfaces(loaded.surfaces))
}

/// CLI entry point for the generic `runweaver` binary: every command that
/// needs project config loads `.runweaver/manifest.json` with the default
/// builtin registry; manifest-free commands (`init`, `manifest`, ...) run
/// without one.
pub fn run_generic_runweaver_cli(args: &[String], io: RunweaverCliIo<'_>) -> Result<i32> {
    let options = parse_runweaver_options(args.get(1..).unwrap_or_default())?;
    let root = absolute_path(options.cwd.as_deref().unwrap_or("."));

    let load_config = |request: LoadRunweaverConfigRequest<'_>| {
        Ok(load_generic_runweaver_project(request.cwd)?
            .runweaver_config()
            .clone())
    };
    let load_hooks = |request: LoadRunweaverAgentHooksConfigRequest<'_>| {
        load_generic_runweaver_project(request.root)?
            .agent_hooks_config()
            .cloned()
            .ok_or_else(|| {
                anyhow!("The Runweaver manifest does not configure a surfaces.agents entry.")
            })
    };
    let compile_binary = |_request: CompileRunweaverBinaryRequest<'_>| {
        Err(anyhow!(
            "The generic runweaver binary cannot compile a project binary; add a project-specific Rust crate that embeds the runweaver library."
        ))
    };
    let generated_surface_files = || {
        Ok(load_generic_runweaver_project(&root)?
            .generated_surface_files()
            .to_vec())
    };
    let git_surface = || {
        Ok(load_generic_runweaver_project(&root)?
            .git_surface()
            .cloned())
    };

    run_runweaver_cli(
        &compiled_cli_args(args),
        RunweaverCliRuntime {
            load_runweaver_config: &load_config,
            load_agent_hooks_config: &load_hooks,
            compile_binary: &compile_binary,
            generated_surface_files: &generated_surface_files,
            git_surface: &git_surface,
        },
        io,
    )
}

fn compiled_cli_args(args: &[String]) -> Vec<String> {
    let mut compiled_args = args.to_vec();
    if compiled_cli_needs_config(args) && !cli_args_have_option(args, "--config") {
        compiled_args.push("--config".to_owned());
        compiled_args.push("<compiled>".to_owned());
    }
    compiled_args
}

fn compiled_cli_needs_config(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("hook" | "sync") => true,
        Some("check") => args.iter().skip(1).any(|arg| arg == "hooks"),
        _ => false,
    }
}

fn cli_args_have_option(args: &[String], option: &str) -> bool {
    args.iter().any(|arg| arg == option)
}

pub fn parse_runweaver_options(args: &[String]) -> Result<RunweaverParsedOptions> {
    let mut positionals = Vec::new();
    let mut files = Vec::new();
    let mut cwd = None;
    let mut config_path = None;
    let mut input_json = None;
    let mut export_name = None;
    let mut out_path = None;
    let mut fingerprint_roots = Vec::new();
    let mut json = RunweaverJsonMode::Off;
    let mut verbose = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--cwd" => {
                cwd = args.get(index + 1).cloned();
                index += 2;
            }
            "--config" => {
                config_path = args.get(index + 1).cloned();
                index += 2;
            }
            "--export" => {
                export_name = args.get(index + 1).cloned();
                index += 2;
            }
            "--out" => {
                out_path = args.get(index + 1).cloned();
                index += 2;
            }
            "--fingerprint" => {
                if let Some(root) = args.get(index + 1) {
                    fingerprint_roots.push(root.clone());
                }
                index += 2;
            }
            "--json" => {
                json = RunweaverJsonMode::Compact;
                index += 1;
            }
            "--input-json" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(anyhow!("Missing --input-json value."));
                };
                input_json = Some(value.clone());
                index += 2;
            }
            "--verbose" => {
                verbose = true;
                index += 1;
            }
            "--file" => {
                if let Some(file) = args.get(index + 1) {
                    files.push(file.clone());
                }
                index += 2;
            }
            "--files" => {
                if let Some(value) = args.get(index + 1) {
                    files.extend(value.split(',').map(str::to_owned));
                }
                index += 2;
            }
            _ if arg.starts_with("--json=") => {
                json = parse_json_mode(&arg["--json=".len()..])?;
                index += 1;
            }
            _ => {
                positionals.push(arg.clone());
                index += 1;
            }
        }
    }

    Ok(RunweaverParsedOptions {
        cwd,
        config_path,
        export_name,
        json,
        verbose,
        files,
        input_json,
        out_path,
        fingerprint_roots,
        positionals,
    })
}

pub fn runweaver_help_text() -> &'static str {
    "runweaver <command>\n\nCommands:\n  init                Scaffold .runweaver/ managed toolchain files\n  install             Install .runweaver/package.json\n  check               Validate compiled config, scaffold, and managed toolchain\n  check hooks         Check generated native agent/git hook config drift\n  check manifest      Check .runweaver/manifest.json drift from stdin JSON\n  compile binary      Compile config + agent hooks into a project binary\n  sync hooks          Write generated native agent/git hook configs\n  sync manifest       Write .runweaver/manifest.json from stdin JSON\n  manifest schema     Write .runweaver/manifest.schema.json\n  manifest types      Write .runweaver/manifest.d.ts TypeScript declarations\n  git-hook <slot>     Execute a configured Git hook slot\n  hook <harness> <command>\n                      Execute a configured agent hook command from stdin\n  run <task>          Execute a named task or tool\n\nOptions:\n  --cwd <path>        Project root\n  --config <path>     Config path relative to cwd\n  --export <name>     Agent-hook config export for hook/sync/check hooks\n  --json[=compact|full]\n                      Print machine-readable JSON. Compact hides successful output.\n  --verbose           Include successful task stdout/stderr. With --json, emit full JSON.\n  --input-json <json|->\n                      Set ExecutionContext.input from a JSON string or stdin.\n  --out <path>        Output path for compile binary. Defaults to .runweaver/bin/runweaver.\n  --fingerprint <path>\n                      Add a source root to compiled binary freshness checks.\n  --file <path>       Add one ExecutionContext file\n  --files <paths>     Add comma-separated ExecutionContext files\n"
}

fn run_runweaver_cli_inner(
    args: &[String],
    runtime: RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let command = args.first().map(String::as_str);
    let options = parse_runweaver_options(&args[1..])?;
    let cwd = absolute_path(options.cwd.as_deref().unwrap_or("."));

    match command {
        Some("init") => run_init(&cwd, io),
        Some("install") => run_install(&cwd, io),
        Some("compile") => run_compile(&cwd, &options, &runtime, io),
        Some("check") => match options.positionals.first().map(String::as_str) {
            Some("hooks") => run_check_hooks(&cwd, &options, &runtime, io),
            Some("manifest") => run_check_manifest(&cwd, io),
            _ => run_check(&cwd, &options, &runtime, io),
        },
        Some("sync") => run_sync(&cwd, &options, &runtime, io),
        Some("manifest") => run_manifest(&cwd, &options, io),
        Some("git-hook") => run_git_hook(&cwd, &options, &runtime, io),
        Some("hook") => run_hook(&cwd, &options, &runtime, io),
        Some("run") => run_named(&cwd, &options, runtime, io),
        Some("help") | None => {
            write!(io.stdout, "{}", runweaver_help_text())?;
            Ok(0)
        }
        Some(command) => {
            write!(
                io.stderr,
                "Unknown runweaver command: {command}\n{}",
                runweaver_help_text()
            )?;
            Ok(1)
        }
    }
}

fn run_init(cwd: &Path, io: &mut RunweaverCliIo<'_>) -> Result<i32> {
    for action in scaffold_runweaver_project(cwd)? {
        writeln!(
            io.stdout,
            "{} {}: {}",
            scaffold_status_label(action.status),
            action.path,
            action.message
        )?;
    }
    Ok(0)
}

fn run_install(cwd: &Path, io: &mut RunweaverCliIo<'_>) -> Result<i32> {
    match install_managed_toolchain(cwd, None) {
        Ok(result) => {
            write!(io.stdout, "{}", result.stdout)?;
            write!(io.stderr, "{}", result.stderr)?;
            Ok(result.exit_code)
        }
        Err(error) => {
            writeln!(io.stderr, "{}", format_diagnostics(&error.diagnostics))?;
            Ok(1)
        }
    }
}

fn run_check(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let config = (runtime.load_runweaver_config)(LoadRunweaverConfigRequest {
        cwd,
        config_path: options.config_path.as_deref(),
    })?;
    let diagnostics = crate::config::validate_project(&config, cwd);
    if options.json != RunweaverJsonMode::Off {
        writeln!(
            io.stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "diagnostics": diagnostics }))?
        )?;
    } else if diagnostics.is_empty() {
        writeln!(io.stdout, "OK")?;
    } else {
        writeln!(io.stderr, "{}", format_diagnostics(&diagnostics))?;
    }
    Ok(if has_error_diagnostics(&diagnostics) {
        1
    } else {
        0
    })
}

fn run_sync(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let target = options.positionals.first().map(String::as_str);
    match target {
        Some("hooks") => {}
        Some("manifest") => return run_sync_manifest(cwd, io),
        _ => {
            write!(
                io.stderr,
                "Unknown runweaver sync target: {}\n{}",
                target.unwrap_or(""),
                runweaver_help_text()
            )?;
            return Ok(1);
        }
    }
    for file in (runtime.generated_surface_files)()? {
        write_generated_surface_file(cwd, &file)?;
        writeln!(io.stdout, "Wrote {}", file.path)?;
    }
    let mut exit_code =
        run_agent_hooks_config_command(AgentHooksConfigCommand::Sync, cwd, options, runtime, io)?;
    if exit_code != 0 {
        exit_code = 1;
    }
    Ok(exit_code)
}

fn run_check_hooks(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let agent_exit_code =
        run_agent_hooks_config_command(AgentHooksConfigCommand::Check, cwd, options, runtime, io)?;
    let generated_files = (runtime.generated_surface_files)()?;
    let generated_mismatches = check_generated_surface_files(cwd, &generated_files)?;
    if generated_mismatches.is_empty() {
        if !generated_files.is_empty() {
            writeln!(io.stdout, "OK generated git/ci surface files")?;
        }
    } else {
        for path in &generated_mismatches {
            writeln!(io.stderr, "Generated surface file drifted: {path}")?;
        }
    }
    Ok(if agent_exit_code == 0 && generated_mismatches.is_empty() {
        0
    } else {
        1
    })
}

/// Reads an authored Runweaver manifest JSON document from CLI stdin, validates
/// it against [`RunweaverDefinitionManifest`], and returns stable pretty JSON.
pub fn read_validated_manifest_stdin(io: &mut RunweaverCliIo<'_>) -> Result<String> {
    let stdin = io.stdin.read()?;
    let manifest = parse_authored_manifest_json(&stdin)?;
    stable_manifest_json(&manifest)
}

/// Parses authored manifest JSON while preserving the original JSON value
/// shape for later stable rendering.
pub fn parse_authored_manifest_json(stdin: &str) -> Result<serde_json::Value> {
    if stdin.trim().is_empty() {
        return Err(anyhow!("Runweaver manifest JSON stdin is empty."));
    }
    let value: serde_json::Value = serde_json::from_str(stdin)
        .map_err(|error| anyhow!("Invalid Runweaver manifest JSON syntax: {error}"))?;
    serde_json::from_value::<RunweaverDefinitionManifest>(value.clone()).map_err(|error| {
        anyhow!("Invalid Runweaver manifest definition shape while validating stdin: {error}")
    })?;
    Ok(value)
}

/// Renders a manifest JSON value with recursively sorted object keys, two-space
/// indentation, and a trailing newline.
pub fn stable_manifest_json(value: &serde_json::Value) -> Result<String> {
    let sorted = stable_json_value(value);
    Ok(serde_json::to_string_pretty(&sorted)? + "\n")
}

fn run_sync_manifest(cwd: &Path, io: &mut RunweaverCliIo<'_>) -> Result<i32> {
    let content = read_validated_manifest_stdin(io)?;
    let path = manifest_artifact_path(cwd);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(0)
}

fn run_check_manifest(cwd: &Path, io: &mut RunweaverCliIo<'_>) -> Result<i32> {
    let expected = read_validated_manifest_stdin(io)?;
    let path = manifest_artifact_path(cwd);
    let actual = std::fs::read_to_string(&path).map_err(|error| {
        anyhow!(
            "Failed to read current Runweaver manifest artifact {}: {error}",
            path.display()
        )
    })?;
    if actual == expected {
        return Ok(0);
    }

    let (actual_path, expected_path) = write_manifest_diff_files(&actual, &expected)?;
    writeln!(
        io.stderr,
        "Runweaver manifest drift detected. Compare with: diff -u {} {}",
        actual_path.display(),
        expected_path.display()
    )?;
    Ok(1)
}

fn manifest_artifact_path(cwd: &Path) -> PathBuf {
    cwd.join(".runweaver/manifest.json")
}

fn run_manifest(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let target = options.positionals.first().map(String::as_str);
    let (relative_path, content) = match target {
        Some("schema") => (
            ".runweaver/manifest.schema.json",
            crate::config::runweaver_manifest_schema_content()?,
        ),
        Some("types") => (".runweaver/manifest.d.ts", MANIFEST_TYPES_DTS.to_owned()),
        _ => {
            write!(
                io.stderr,
                "Unknown runweaver manifest target: {}\n{}",
                target.unwrap_or(""),
                runweaver_help_text()
            )?;
            return Ok(1);
        }
    };
    let path = cwd.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    writeln!(io.stdout, "Wrote {relative_path}")?;
    Ok(0)
}

fn stable_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(stable_json_value).collect())
        }
        serde_json::Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), stable_json_value(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        _ => value.clone(),
    }
}

fn write_manifest_diff_files(actual: &str, expected: &str) -> Result<(PathBuf, PathBuf)> {
    let base = unique_temp_file_base("runweaver-manifest-drift")?;
    let actual_path = base.with_extension("actual.json");
    let expected_path = base.with_extension("expected.json");
    std::fs::write(&actual_path, actual)?;
    std::fs::write(&expected_path, expected)?;
    Ok((actual_path, expected_path))
}

fn unique_temp_file_base(label: &str) -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| anyhow!("Failed to create temp manifest diff path timestamp: {error}"))?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("{label}-{}-{nanos}", std::process::id())))
}

fn run_compile(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let target = options.positionals.first().map(String::as_str);
    if target != Some("binary") {
        write!(
            io.stderr,
            "Unknown runweaver compile target: {}\n{}",
            target.unwrap_or(""),
            runweaver_help_text()
        )?;
        return Ok(1);
    }
    let out_path = options
        .out_path
        .as_deref()
        .unwrap_or(".runweaver/bin/runweaver");
    let result = (runtime.compile_binary)(CompileRunweaverBinaryRequest {
        cwd,
        config_path: options.config_path.as_deref(),
        export_name: options.export_name.as_deref(),
        out_path,
        fingerprint_roots: &options.fingerprint_roots,
    })?;
    writeln!(
        io.stdout,
        "Wrote {} ({} inputs, {})",
        relative_path(cwd, &result.outfile),
        result.manifest.input_count,
        result.manifest.fingerprint
    )?;
    Ok(0)
}

fn run_hook(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let Some(config_path) = options.config_path.as_deref() else {
        writeln!(io.stderr, "hook requires --config <path>.")?;
        return Ok(1);
    };
    let config = (runtime.load_agent_hooks_config)(LoadRunweaverAgentHooksConfigRequest {
        root: cwd,
        config_path,
        export_name: options.export_name.as_deref(),
    })?;
    let stdin = io.stdin.read()?;
    let mut reader = Cursor::new(stdin);
    run_agent_hooks_process_main(
        &config.app,
        &options.positionals,
        AgentHooksProcessIo {
            stdin: &mut reader,
            env: io.env,
            stdout: io.stdout,
            stderr: io.stderr,
        },
    )
}

fn run_git_hook(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let Some(slot) = options.positionals.first().map(String::as_str) else {
        writeln!(io.stderr, "Missing git hook slot.")?;
        return Ok(1);
    };
    let Some(git) = (runtime.git_surface)()? else {
        writeln!(io.stderr, "No surfaces.git manifest is configured.")?;
        return Ok(1);
    };
    let config = (runtime.load_runweaver_config)(LoadRunweaverConfigRequest {
        cwd,
        config_path: options.config_path.as_deref(),
    })?;
    match slot {
        "pre-commit" => run_git_pre_commit(cwd, &config, &git, io),
        "commit-msg" => {
            let Some(message_file) = options.positionals.get(1) else {
                writeln!(
                    io.stderr,
                    "commit-msg requires Git's message file argument."
                )?;
                return Ok(1);
            };
            let Some(slot) = &git.commit_msg else {
                writeln!(io.stderr, "surfaces.git.commitMsg is not configured.")?;
                return Ok(1);
            };
            run_git_named(
                cwd,
                &config,
                &slot.tool,
                std::slice::from_ref(message_file),
                &[],
                io,
            )
        }
        "pre-push" => {
            let Some(slot) = &git.pre_push else {
                writeln!(io.stderr, "surfaces.git.prePush is not configured.")?;
                return Ok(1);
            };
            run_git_named(cwd, &config, &slot.run, &[], &[], io)
        }
        "post-commit" => {
            let Some(slot) = &git.post_commit else {
                writeln!(io.stderr, "surfaces.git.postCommit is not configured.")?;
                return Ok(1);
            };
            run_git_named(cwd, &config, &slot.tool, &[], &[], io)
        }
        _ => {
            writeln!(io.stderr, "Unknown git hook slot: {slot}")?;
            Ok(1)
        }
    }
}

fn run_git_pre_commit(
    cwd: &Path,
    config: &RunweaverConfig,
    git: &GitSurfaceManifest,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let Some(slot) = &git.pre_commit else {
        writeln!(io.stderr, "surfaces.git.preCommit is not configured.")?;
        return Ok(1);
    };
    let files = if slot.files == Some(GitFilesScopeManifest::Staged) {
        staged_files(cwd)?
    } else {
        Vec::new()
    };
    let exit_code = run_git_named(cwd, config, &slot.run, &[], &files, io)?;
    if exit_code != 0 {
        return Ok(exit_code);
    }
    if slot.files == Some(GitFilesScopeManifest::Staged) && !files.is_empty() {
        restage_files(cwd, &files)?;
    }
    for tool in &slot.also {
        let exit_code = run_git_named(cwd, config, tool, &[], &[], io)?;
        if exit_code != 0 {
            return Ok(exit_code);
        }
    }
    Ok(0)
}

fn run_git_named(
    cwd: &Path,
    config: &RunweaverConfig,
    task_name: &str,
    args: &[String],
    files: &[String],
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let mut context_options = CreateExecutionContextOptions::new(path_to_string(cwd));
    context_options.env = Some(hook_env_to_hash_map(io.env));
    context_options.files = files.to_vec();
    if !args.is_empty() {
        context_options.input = Some(serde_json::json!({ "args": args }));
    }
    let run = run_task(config, task_name, create_execution_context(context_options))?;
    write_git_projection(task_name, &run, io)?;
    Ok(if is_blocking_run(&run) { 1 } else { 0 })
}

fn write_git_projection(task_name: &str, run: &TaskRun, io: &mut RunweaverCliIo<'_>) -> Result<()> {
    if is_blocking_run(run) {
        writeln!(io.stderr, "{task_name}: {}", task_run_result_label(run))?;
        write!(io.stderr, "{}", format_notable_runs(run))?;
    } else {
        writeln!(io.stdout, "{task_name}: {}", task_run_result_label(run))?;
    }
    Ok(())
}

fn run_agent_hooks_config_command(
    command: AgentHooksConfigCommand,
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: &RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let command_label = match command {
        AgentHooksConfigCommand::Sync => "sync",
        AgentHooksConfigCommand::Check => "check",
        AgentHooksConfigCommand::Help => "help",
    };
    let Some(config_path) = options.config_path.as_deref() else {
        writeln!(io.stderr, "{command_label} hooks requires --config <path>.")?;
        return Ok(1);
    };
    let mut args = vec![
        command_label.to_owned(),
        "--config".to_owned(),
        config_path.to_owned(),
    ];
    if let Some(export_name) = &options.export_name {
        args.extend(["--export".to_owned(), export_name.clone()]);
    }
    let load_config = |request: AgentHooksConfigCliLoadRequest<'_>| {
        (runtime.load_agent_hooks_config)(LoadRunweaverAgentHooksConfigRequest {
            root: request.root,
            config_path: request.config_path,
            export_name: request.export_name,
        })
    };

    run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
        args: &args,
        root: cwd,
        stdout: io.stdout,
        stderr: io.stderr,
        load_config: &load_config,
    })
}

fn run_named(
    cwd: &Path,
    options: &RunweaverParsedOptions,
    runtime: RunweaverCliRuntime<'_, '_>,
    io: &mut RunweaverCliIo<'_>,
) -> Result<i32> {
    let Some(task_name) = options.positionals.first() else {
        writeln!(io.stderr, "Missing task name.")?;
        return Ok(1);
    };

    let config = (runtime.load_runweaver_config)(LoadRunweaverConfigRequest {
        cwd,
        config_path: options.config_path.as_deref(),
    })?;
    let input = parse_input_json(options, &mut io.stdin)?;
    let mut context_options = CreateExecutionContextOptions::new(path_to_string(cwd));
    context_options.env = Some(hook_env_to_hash_map(io.env));
    context_options.files = options.files.clone();
    let trailing_args = options
        .positionals
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();
    if input.is_some() {
        context_options.input = input;
    } else if !trailing_args.is_empty() {
        context_options.input = Some(serde_json::json!({ "args": trailing_args }));
    }
    let run = run_task(
        &config,
        task_name,
        create_execution_context(context_options),
    )?;

    if options.json != RunweaverJsonMode::Off {
        let payload = if options.json == RunweaverJsonMode::Full || options.verbose {
            serde_json::to_value(&run)?
        } else {
            serde_json::to_value(compact_run_for_agents(&run))?
        };
        writeln!(io.stdout, "{}", serde_json::to_string_pretty(&payload)?)?;
    } else {
        writeln!(
            io.stdout,
            "{}: {}",
            run.task_name,
            task_run_result_label(&run)
        )?;
        if options.verbose && run.status == TaskRunStatus::Completed {
            if let Some(output) = &run.output {
                write!(io.stdout, "{}", output.stdout)?;
                write!(io.stderr, "{}", output.stderr)?;
                if let Some(error) = &output.error {
                    writeln!(io.stderr, "{error}")?;
                }
            }
        } else {
            write!(io.stderr, "{}", format_notable_runs(&run))?;
        }
        if run.status != TaskRunStatus::Completed
            && options.verbose
            && let Some(reason) = &run.reason
        {
            writeln!(io.stderr, "{reason}")?;
        }
    }

    Ok(if is_blocking_run(&run) { 1 } else { 0 })
}

fn write_generated_surface_file(cwd: &Path, file: &GeneratedSurfaceFile) -> Result<()> {
    let path = cwd.join(&file.path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &file.content)?;
    set_executable_if_needed(&path, file.executable)?;
    Ok(())
}

fn check_generated_surface_files(
    cwd: &Path,
    files: &[GeneratedSurfaceFile],
) -> Result<Vec<String>> {
    let mut mismatches = Vec::new();
    for file in files {
        let path = cwd.join(&file.path);
        match std::fs::read_to_string(&path) {
            Ok(actual) if actual == file.content => {}
            _ => mismatches.push(file.path.clone()),
        }
    }
    Ok(mismatches)
}

fn set_executable_if_needed(path: &Path, executable: bool) -> Result<()> {
    if !executable {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn staged_files(cwd: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only", "--diff-filter=ACMR"])
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff --cached failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn restage_files(cwd: &Path, files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let output = std::process::Command::new("git")
        .arg("add")
        .arg("--")
        .args(files)
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn parse_input_json(
    options: &RunweaverParsedOptions,
    stdin: &mut RunweaverStdin<'_>,
) -> Result<Option<serde_json::Value>> {
    let Some(input_json) = &options.input_json else {
        return Ok(None);
    };
    let source = if input_json == "-" {
        stdin.read()?
    } else {
        input_json.clone()
    };
    serde_json::from_str(&source)
        .map(Some)
        .map_err(|error| anyhow!("Invalid --input-json payload: {error}"))
}

fn parse_json_mode(value: &str) -> Result<RunweaverJsonMode> {
    match value {
        "compact" => Ok(RunweaverJsonMode::Compact),
        "full" => Ok(RunweaverJsonMode::Full),
        _ => Err(anyhow!("Unsupported --json mode: {value}")),
    }
}

fn scaffold_status_label(status: ScaffoldActionStatus) -> &'static str {
    match status {
        ScaffoldActionStatus::Created => "created",
        ScaffoldActionStatus::Skipped => "skipped",
        ScaffoldActionStatus::Changed => "changed",
    }
}

fn hook_env_to_hash_map(env: &HookEnv) -> HashMap<String, String> {
    env.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn absolute_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    use crate::bindings::bind;
    use crate::config::{
        ActionResult, ActionTask, ExecutionContext, ParallelTask, RunweaverDefinition,
        TaskCompletion, TaskDefinition, TaskKind, TaskOutput, ToolDefinition, define_config,
    };
    use crate::embedded::RunweaverBinaryManifest;
    use crate::services::test_support::TestPorts;
    use crate::surfaces::SurfaceTrigger;
    use crate::surfaces::agent_hooks::{
        AgentHooksConfigDefinition, AgentHooksConfigHook, Harness, HarnessCodec, HarnessDefinition,
        HarnessHookConfig, HarnessHookConfigRenderInput, HarnessTarget, HookBinding,
        HookCommandSpec, HookEmission, HookEvent, HookOutcome, HookRequest, HookStage,
        define_agent_hooks_config, define_harness, hook_groups_by_stage,
    };

    use super::*;

    struct CustomCodec;

    impl HarnessCodec for CustomCodec {
        fn harness(&self) -> &'static str {
            "custom"
        }

        fn decode(&self, stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
            let payload: serde_json::Value = serde_json::from_str(stdin)?;
            Ok(HookRequest {
                event: HookEvent {
                    harness: "custom".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/repo".to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: payload
                        .get("command")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(&self, outcome: HookOutcome, _request: &HookRequest) -> HookEmission {
            HookEmission {
                exit_code: 0,
                stdout: Some(serde_json::to_string(&outcome).unwrap()),
                stderr: None,
            }
        }

        fn encode_failure(&self, _stage: HookStage, error: &anyhow::Error) -> HookEmission {
            HookEmission {
                exit_code: 1,
                stdout: None,
                stderr: Some(format!("{error}\n")),
            }
        }
    }

    static CUSTOM_CODEC: CustomCodec = CustomCodec;

    fn echo_input_action(ctx: &ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Success,
            output: TaskOutput::success(),
            data: ctx.input.clone(),
            next_context: None,
        }
    }

    fn failing_action(_: &ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Error,
            output: TaskOutput {
                exit_code: Some(1),
                stdout: "bad stdout\n".to_owned(),
                stderr: "bad stderr\n".to_owned(),
                error: None,
            },
            data: None,
            next_context: None,
        }
    }

    fn runweaver_config() -> RunweaverConfig {
        define_config(RunweaverConfig {
            tools: HashMap::<String, ToolDefinition>::new(),
            policies: HashMap::new(),
            tasks: HashMap::from([
                (
                    "echoInput".to_owned(),
                    TaskDefinition::Action(ActionTask::new(echo_input_action)),
                ),
                (
                    "fail".to_owned(),
                    TaskDefinition::Action(ActionTask::new(failing_action)),
                ),
                (
                    "check".to_owned(),
                    TaskDefinition::Parallel(ParallelTask {
                        refs: vec!["echoInput".to_owned(), "fail".to_owned()],
                        fail_fast: false,
                        policies: Vec::new(),
                    }),
                ),
            ]),
        })
    }

    fn custom_harness() -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: "custom".to_owned(),
            codec: &CUSTOM_CODEC,
            hook_config: HarnessHookConfig::new(
                ".custom/hooks.json",
                |input: HarnessHookConfigRenderInput<'_>| {
                    Ok(format!(
                        "{}\n",
                        serde_json::to_string_pretty(&hook_groups_by_stage(input.groups)).unwrap()
                    ))
                },
            ),
            agents_surface: crate::surfaces::agent_hooks::AgentsSurfaceDefaults::new(
                crate::surfaces::agent_hooks::RunweaverHookCommandCwd::None,
            ),
        })
    }

    fn agent_hooks_config() -> AgentHooksConfig<'static> {
        let harness = custom_harness();
        define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness.clone()],
            vec![HarnessTarget::new(
                "custom",
                ".custom/hooks.json",
                "fixture-runweaver custom",
            )],
            vec![AgentHooksConfigHook {
                command: HookCommandSpec::new("guard", HookStage::PreTool, |event| {
                    Ok(HookOutcome::block(format!(
                        "blocked {}",
                        event.tool_command.as_deref().unwrap_or("")
                    )))
                })
                .with_harnesses(["custom"]),
                bindings: vec![HookBinding::new("custom", 10, "Guard").with_matcher("Bash")],
            }],
        ))
        .unwrap()
    }

    fn manifest(input_count: usize) -> RunweaverBinaryManifest {
        RunweaverBinaryManifest {
            version: 1,
            fingerprint: "sha256-fixture".to_owned(),
            source_roots: Vec::new(),
            input_count,
            inputs: Vec::new(),
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        }
    }

    struct CapturedIo {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        env: HookEnv,
    }

    impl CapturedIo {
        fn new() -> Self {
            Self {
                stdout: Vec::new(),
                stderr: Vec::new(),
                env: HookEnv::new(),
            }
        }

        fn io<'a>(&'a mut self, stdin: &'a str) -> RunweaverCliIo<'a> {
            RunweaverCliIo {
                stdin: RunweaverStdin::Text(stdin),
                stdout: &mut self.stdout,
                stderr: &mut self.stderr,
                env: &self.env,
            }
        }

        fn stdout(&self) -> String {
            String::from_utf8(self.stdout.clone()).unwrap()
        }

        fn stderr(&self) -> String {
            String::from_utf8(self.stderr.clone()).unwrap()
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "runweaver-cli-rs-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn valid_project(root: &Path) {
        fs::create_dir_all(root.join(".runweaver/node_modules/.bin")).unwrap();
        fs::create_dir_all(root.join(".runweaver/configs")).unwrap();
        fs::write(root.join(".runweaver/package.json"), "{\"private\":true}\n").unwrap();
        fs::write(root.join(".runweaver/bun.lock"), "").unwrap();
        fs::write(root.join(".gitignore"), ".runweaver/node_modules/\n").unwrap();
    }

    fn valid_manifest_json_unsorted() -> &'static str {
        r#"{"version":2,"tools":{},"pipelines":{},"operations":{},"bindings":[]}"#
    }

    fn valid_manifest_json_stable() -> &'static str {
        "{\n  \"bindings\": [],\n  \"operations\": {},\n  \"pipelines\": {},\n  \"tools\": {},\n  \"version\": 2\n}\n"
    }

    fn run_manifest_cli(
        root: &Path,
        argv: &[&str],
        stdin: &str,
        captured: &mut CapturedIo,
    ) -> Result<i32> {
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };

        run_runweaver_cli(
            &args(argv),
            RunweaverCliRuntime {
                load_runweaver_config: &load_config,
                load_agent_hooks_config: &load_hooks,
                compile_binary: &compile,
                generated_surface_files: &empty_generated_surface_files,
                git_surface: &empty_git_surface,
            },
            captured.io(stdin),
        )
    }

    fn empty_generated_surface_files() -> Result<Vec<GeneratedSurfaceFile>> {
        Ok(Vec::new())
    }

    fn empty_git_surface() -> Result<Option<GitSurfaceManifest>> {
        Ok(None)
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn runweaver_cli_parses_shared_options() {
        let parsed = parse_runweaver_options(&args(&[
            "task",
            "--cwd",
            "/repo",
            "--config",
            "custom.ts",
            "--json=full",
            "--file",
            "src/a.ts",
            "--files",
            "src/b.ts,src/c.ts",
            "--fingerprint",
            "src",
        ]))
        .unwrap();

        assert_eq!(parsed.cwd.as_deref(), Some("/repo"));
        assert_eq!(parsed.config_path.as_deref(), Some("custom.ts"));
        assert_eq!(parsed.json, RunweaverJsonMode::Full);
        assert_eq!(parsed.files, vec!["src/a.ts", "src/b.ts", "src/c.ts"]);
        assert_eq!(parsed.fingerprint_roots, vec!["src"]);
        assert_eq!(parsed.positionals, vec!["task"]);
    }

    #[test]
    fn runweaver_cli_init_reports_scaffold_actions() {
        let root = temp_root("init");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };
        let mut captured = CapturedIo::new();

        let exit_code = run_runweaver_cli(
            &args(&["init", "--cwd", root.to_str().unwrap()]),
            RunweaverCliRuntime {
                load_runweaver_config: &load_config,
                load_agent_hooks_config: &load_hooks,
                compile_binary: &compile,
                generated_surface_files: &empty_generated_surface_files,
                git_surface: &empty_git_surface,
            },
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert!(captured.stdout().contains("created .runweaver"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_check_uses_injected_config_loader() {
        let root = temp_root("check");
        valid_project(&root);
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_calls = Cell::new(0);
        let load_config = |request: LoadRunweaverConfigRequest<'_>| {
            load_calls.set(load_calls.get() + 1);
            assert_eq!(request.cwd, root.as_path());
            assert_eq!(request.config_path, None);
            Ok(config.clone())
        };
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };
        let mut captured = CapturedIo::new();

        let exit_code = run_runweaver_cli(
            &args(&["check", "--cwd", root.to_str().unwrap()]),
            RunweaverCliRuntime {
                load_runweaver_config: &load_config,
                load_agent_hooks_config: &load_hooks,
                compile_binary: &compile,
                generated_surface_files: &empty_generated_surface_files,
                git_surface: &empty_git_surface,
            },
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(captured.stdout(), "OK\n");
        assert_eq!(load_calls.get(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_runs_tasks_with_json_input_and_compact_output() {
        let root = temp_root("run");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };
        let mut full = CapturedIo::new();
        let mut compact = CapturedIo::new();

        assert_eq!(
            run_runweaver_cli(
                &args(&[
                    "run",
                    "echoInput",
                    "--cwd",
                    root.to_str().unwrap(),
                    "--json=full",
                    "--input-json",
                    "-",
                ]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                full.io(r#"{"path":"src/index.ts"}"#),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&full.stdout()).unwrap()["data"],
            serde_json::json!({ "path": "src/index.ts" })
        );

        assert_eq!(
            run_runweaver_cli(
                &args(&["run", "check", "--cwd", root.to_str().unwrap(), "--json"]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                compact.io(""),
            )
            .unwrap(),
            1
        );
        let compact_json = serde_json::from_str::<serde_json::Value>(&compact.stdout()).unwrap();
        assert_eq!(compact_json["taskName"], "check");
        assert_eq!(compact_json["children"][0]["taskName"], "fail");
        assert!(!compact_json.to_string().contains("echoInput"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_restaging_helper_re_adds_fixed_staged_files() {
        let root = temp_root("restage");
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Test User"]);
        fs::write(root.join("file.txt"), "one\n").unwrap();
        run_git(&root, &["add", "file.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);

        fs::write(root.join("file.txt"), "two\n").unwrap();
        run_git(&root, &["add", "file.txt"]);
        fs::write(root.join("file.txt"), "three\n").unwrap();
        restage_files(&root, &["file.txt".to_owned()]).unwrap();

        let staged = std::process::Command::new("git")
            .args(["show", ":file.txt"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&staged.stdout), "three\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_projection_writes_failures_to_stderr_for_abort() {
        let mut captured = CapturedIo::new();
        let run = TaskRun {
            task_name: "lint".to_owned(),
            task_type: TaskKind::Action,
            status: TaskRunStatus::Completed,
            completion: Some(TaskCompletion::Error),
            reason: None,
            output: Some(TaskOutput {
                exit_code: Some(1),
                stdout: String::new(),
                stderr: "bad\n".to_owned(),
                error: None,
            }),
            data: None,
            children: Vec::new(),
            next_context: None,
        };

        write_git_projection("lint", &run, &mut captured.io("")).unwrap();

        assert!(captured.stderr().contains("lint: error"));
        assert!(captured.stderr().contains("bad"));
        assert_eq!(captured.stdout(), "");
    }

    #[test]
    fn run_compiled_runweaver_cli_runs_tasks_without_dynamic_config_loader() {
        let root = temp_root("compiled-run");
        let config = runweaver_config();
        let mut captured = CapturedIo::new();

        let exit_code = run_compiled_runweaver_cli(
            &args(&[
                "run",
                "echoInput",
                "--cwd",
                root.to_str().unwrap(),
                "--json=full",
                "--input-json",
                r#"{"from":"compiled"}"#,
            ]),
            &config,
            None,
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&captured.stdout()).unwrap()["data"],
            serde_json::json!({ "from": "compiled" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_compiled_runweaver_project_cli_runs_tasks_from_compiled_project() {
        let root = temp_root("compiled-project-run");
        let task_config = runweaver_config();
        let task_operation = task_config
            .tasks
            .get("echoInput")
            .expect("fixture task should exist")
            .clone();
        let binding = bind(SurfaceTrigger {
            surface: "cli".to_owned(),
            name: "echo".to_owned(),
            phase: None,
        })
        .to("echoInput")
        .finish();
        let definition = RunweaverDefinition::from(task_config)
            .with_operation("echoInput", task_operation)
            .with_binding(binding.clone());
        let project = CompiledRunweaverProject::new(definition);
        let mut captured = CapturedIo::new();

        let exit_code = run_compiled_runweaver_project_cli(
            &args(&[
                "run",
                "echoInput",
                "--cwd",
                root.to_str().unwrap(),
                "--json=full",
                "--input-json",
                r#"{"from":"compiled-project"}"#,
            ]),
            &project,
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&captured.stdout()).unwrap()["data"],
            serde_json::json!({ "from": "compiled-project" })
        );
        assert!(project.runweaver_config().tasks.contains_key("echoInput"));
        assert!(
            project
                .runweaver_definition()
                .tasks
                .contains_key("echoInput")
        );
        assert!(
            project
                .runweaver_definition()
                .operations
                .contains_key("echoInput")
        );
        assert_eq!(
            project.runweaver_definition().bindings[0].operation_name,
            "echoInput"
        );
        let ports = TestPorts::default();
        let operation_output = project
            .run_operation_as_json(
                "echoInput",
                serde_json::json!({ "from": "compiled-project-method" }),
                ExecutionContext::new(root.to_string_lossy().into_owned()),
                &ports.services(),
            )
            .unwrap();
        let mut binding_context = serde_json::json!({});
        let binding_output = project
            .run_bound_operation(
                &binding,
                serde_json::json!({ "from": "compiled-project-binding" }),
                &mut binding_context,
                ExecutionContext::new(root.to_string_lossy().into_owned()),
                &ports.services(),
            )
            .unwrap();

        assert_eq!(
            operation_output["data"],
            serde_json::json!({ "from": "compiled-project-method" })
        );
        assert_eq!(
            binding_output["data"],
            serde_json::json!({ "from": "compiled-project-binding" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_compiled_runweaver_project_cli_syncs_checks_and_runs_hooks_from_project_builder() {
        let root = temp_root("compiled-project-hooks");
        let project = compiled_runweaver_project(runweaver_config())
            .agent_hooks_config_with(
                "fixture-hooks",
                "fixture-runweaver",
                "fixture.rs",
                |hooks| {
                    let harness = custom_harness();
                    hooks.harness(harness.clone());
                    hooks.target(HarnessTarget::new(
                        "custom",
                        ".custom/hooks.json",
                        "fixture-runweaver custom",
                    ));
                    hooks.hook(
                        HookCommandSpec::new("guard", HookStage::PreTool, |event| {
                            Ok(HookOutcome::block(format!(
                                "blocked {}",
                                event.tool_command.as_deref().unwrap_or("")
                            )))
                        }),
                        [HookBinding::new("custom", 10, "Guard").with_matcher("Bash")],
                    );
                },
            )
            .unwrap()
            .build();

        let mut sync = CapturedIo::new();
        assert_eq!(
            run_compiled_runweaver_project_cli(
                &args(&["sync", "hooks", "--cwd", root.to_str().unwrap()]),
                &project,
                sync.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(sync.stdout(), "Wrote .custom/hooks.json\n");

        let mut check = CapturedIo::new();
        assert_eq!(
            run_compiled_runweaver_project_cli(
                &args(&["check", "hooks", "--cwd", root.to_str().unwrap()]),
                &project,
                check.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(check.stdout(), "OK harness hook config files\n");

        let mut hook = CapturedIo::new();
        assert_eq!(
            run_compiled_runweaver_project_cli(
                &args(&["hook", "custom", "guard", "--cwd", root.to_str().unwrap()]),
                &project,
                hook.io(r#"{"command":"rm -rf node_modules"}"#),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&hook.stdout()).unwrap(),
            serde_json::json!({ "status": "block", "reason": "blocked rm -rf node_modules" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_compiled_runweaver_cli_fails_closed_without_compiled_hook_config() {
        let root = temp_root("compiled-hook-missing");
        let config = runweaver_config();
        let mut captured = CapturedIo::new();

        let error = run_compiled_runweaver_cli(
            &args(&["hook", "custom", "guard", "--cwd", root.to_str().unwrap()]),
            &config,
            None,
            captured.io(r#"{"command":"pwd"}"#),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Compiled Runweaver CLI was called without compiled agent hook config."
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_compiled_runweaver_cli_syncs_and_checks_hooks_without_config_arg() {
        let root = temp_root("compiled-hooks");
        let config = runweaver_config();
        let hooks = agent_hooks_config();

        let mut sync = CapturedIo::new();
        assert_eq!(
            run_compiled_runweaver_cli(
                &args(&["sync", "hooks", "--cwd", root.to_str().unwrap()]),
                &config,
                Some(&hooks),
                sync.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(sync.stdout(), "Wrote .custom/hooks.json\n");

        let mut check = CapturedIo::new();
        assert_eq!(
            run_compiled_runweaver_cli(
                &args(&["check", "hooks", "--cwd", root.to_str().unwrap()]),
                &config,
                Some(&hooks),
                check.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(check.stdout(), "OK harness hook config files\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_compiled_runweaver_cli_with_compile_delegates_without_config_or_export_arg() {
        let root = temp_root("compiled-compile");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let compile_calls = Cell::new(0);
        let compile = |request: CompileRunweaverBinaryRequest<'_>| {
            compile_calls.set(compile_calls.get() + 1);
            assert_eq!(request.cwd, root.as_path());
            assert_eq!(request.config_path, None);
            assert_eq!(request.export_name, None);
            assert_eq!(request.out_path, ".runweaver/bin/runweaver");
            assert_eq!(request.fingerprint_roots, &["src".to_owned()]);
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(3),
            })
        };
        let mut captured = CapturedIo::new();

        let exit_code = run_compiled_runweaver_cli_with_compile(
            &args(&[
                "compile",
                "binary",
                "--cwd",
                root.to_str().unwrap(),
                "--fingerprint",
                "src",
            ]),
            &config,
            Some(&hooks),
            &compile,
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            captured.stdout(),
            "Wrote .runweaver/bin/runweaver (3 inputs, sha256-fixture)\n"
        );
        assert_eq!(compile_calls.get(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_syncs_checks_and_runs_agent_hooks() {
        let root = temp_root("hooks");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |request: LoadRunweaverAgentHooksConfigRequest<'_>| {
            assert_eq!(request.root, root.as_path());
            assert_eq!(request.config_path, "runweaver.config.ts");
            Ok(hooks.clone())
        };
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };

        let mut sync = CapturedIo::new();
        assert_eq!(
            run_runweaver_cli(
                &args(&[
                    "sync",
                    "hooks",
                    "--cwd",
                    root.to_str().unwrap(),
                    "--config",
                    "runweaver.config.ts",
                ]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                sync.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(sync.stdout(), "Wrote .custom/hooks.json\n");

        let mut check = CapturedIo::new();
        assert_eq!(
            run_runweaver_cli(
                &args(&[
                    "check",
                    "hooks",
                    "--cwd",
                    root.to_str().unwrap(),
                    "--config",
                    "runweaver.config.ts",
                ]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                check.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(check.stdout(), "OK harness hook config files\n");

        let mut hook = CapturedIo::new();
        assert_eq!(
            run_runweaver_cli(
                &args(&[
                    "hook",
                    "custom",
                    "guard",
                    "--cwd",
                    root.to_str().unwrap(),
                    "--config",
                    "runweaver.config.ts",
                ]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                hook.io(r#"{"command":"rm -rf node_modules"}"#),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&hook.stdout()).unwrap(),
            serde_json::json!({ "status": "block", "reason": "blocked rm -rf node_modules" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_sync_manifest_writes_stable_manifest_json_from_stdin() {
        let root = temp_root("sync-manifest");
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["sync", "manifest", "--cwd", root.to_str().unwrap()],
            valid_manifest_json_unsorted(),
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            fs::read_to_string(root.join(".runweaver/manifest.json")).unwrap(),
            valid_manifest_json_stable()
        );
        assert_eq!(captured.stdout(), "");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_sync_manifest_rejects_invalid_manifest_shape_without_writing() {
        let root = temp_root("sync-manifest-invalid-shape");
        let mut captured = CapturedIo::new();

        let error = run_manifest_cli(
            &root,
            &["sync", "manifest", "--cwd", root.to_str().unwrap()],
            r#"{"version":"2","tools":{},"pipelines":{},"operations":{},"bindings":[]}"#,
            &mut captured,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Invalid Runweaver manifest definition shape")
        );
        assert!(!root.join(".runweaver/manifest.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_sync_manifest_rejects_broken_json_without_writing() {
        let root = temp_root("sync-manifest-broken-json");
        let mut captured = CapturedIo::new();

        let error = run_manifest_cli(
            &root,
            &["sync", "manifest", "--cwd", root.to_str().unwrap()],
            r#"{"version":2,"tools":{}"#,
            &mut captured,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Invalid Runweaver manifest JSON syntax")
        );
        assert!(!root.join(".runweaver/manifest.json").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_check_manifest_exits_zero_when_manifest_matches() {
        let root = temp_root("check-manifest-match");
        fs::create_dir_all(root.join(".runweaver")).unwrap();
        fs::write(
            root.join(".runweaver/manifest.json"),
            valid_manifest_json_stable(),
        )
        .unwrap();
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["check", "manifest", "--cwd", root.to_str().unwrap()],
            valid_manifest_json_unsorted(),
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(captured.stdout(), "");
        assert_eq!(captured.stderr(), "");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_check_manifest_reports_diff_paths_when_manifest_drifted() {
        let root = temp_root("check-manifest-drift");
        fs::create_dir_all(root.join(".runweaver")).unwrap();
        fs::write(root.join(".runweaver/manifest.json"), "{}\n").unwrap();
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["check", "manifest", "--cwd", root.to_str().unwrap()],
            valid_manifest_json_unsorted(),
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 1);
        assert!(captured.stderr().contains("diff -u "));
        assert!(captured.stderr().contains(".actual.json"));
        assert!(captured.stderr().contains(".expected.json"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_check_manifest_errors_when_manifest_file_is_missing() {
        let root = temp_root("check-manifest-missing");
        let mut captured = CapturedIo::new();

        let error = run_manifest_cli(
            &root,
            &["check", "manifest", "--cwd", root.to_str().unwrap()],
            valid_manifest_json_unsorted(),
            &mut captured,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to read current Runweaver manifest artifact")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_manifest_schema_writes_schema_artifact() {
        let root = temp_root("manifest-schema");
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["manifest", "schema", "--cwd", root.to_str().unwrap()],
            "",
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        let schema: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join(".runweaver/manifest.schema.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            schema.get("title").and_then(serde_json::Value::as_str),
            Some("RunweaverDefinitionManifest")
        );
        assert_eq!(captured.stdout(), "Wrote .runweaver/manifest.schema.json\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_manifest_types_writes_embedded_declarations() {
        let root = temp_root("manifest-types");
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["manifest", "types", "--cwd", root.to_str().unwrap()],
            "",
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        let types = fs::read_to_string(root.join(".runweaver/manifest.d.ts")).unwrap();
        assert!(types.contains("export interface RunweaverDefinitionManifest"));
        assert!(types.contains("schema-sha256: "));
        assert_eq!(captured.stdout(), "Wrote .runweaver/manifest.d.ts\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_manifest_rejects_unknown_subcommand() {
        let root = temp_root("manifest-unknown");
        let mut captured = CapturedIo::new();

        let exit_code = run_manifest_cli(
            &root,
            &["manifest", "bogus", "--cwd", root.to_str().unwrap()],
            "",
            &mut captured,
        )
        .unwrap();

        assert_eq!(exit_code, 1);
        assert!(
            captured
                .stderr()
                .contains("Unknown runweaver manifest target: bogus")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_manifest_types_are_generated_from_current_schema() {
        let stamped = MANIFEST_TYPES_DTS
            .lines()
            .find_map(|line| line.split("schema-sha256: ").nth(1))
            .expect("embedded manifest.d.ts should carry a schema-sha256 stamp");

        assert_eq!(
            stamped,
            crate::config::runweaver_manifest_schema_sha256(),
            "assets/manifest.d.ts is stale; regenerate it with scripts/generate-manifest-types.sh"
        );
    }

    #[test]
    fn stable_manifest_json_is_idempotent_on_stable_input() {
        let authored = concat!(
            "{\n",
            "  \"bindings\": [],\n",
            "  \"operations\": {},\n",
            "  \"pipelines\": {\n",
            "    \"check\": {\n",
            "      \"check\": [\n",
            "        \"fmt\"\n",
            "      ]\n",
            "    }\n",
            "  },\n",
            "  \"tools\": {\n",
            "    \"fmt\": {\n",
            "      \"args\": [],\n",
            "      \"preset\": \"oxfmt\"\n",
            "    }\n",
            "  },\n",
            "  \"version\": 2\n",
            "}\n",
        );
        let value = parse_authored_manifest_json(authored).unwrap();

        let stable = stable_manifest_json(&value).unwrap();

        assert_eq!(stable, authored);
    }

    #[test]
    fn runweaver_cli_compile_binary_delegates_optional_config_and_export() {
        let root = temp_root("compile");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let compile_calls = Cell::new(0);
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |request: CompileRunweaverBinaryRequest<'_>| {
            let call = compile_calls.get();
            compile_calls.set(call + 1);
            assert_eq!(request.cwd, root.as_path());
            assert_eq!(request.out_path, ".runweaver/bin/runweaver");
            let input_count = if call == 0 {
                assert_eq!(request.config_path, None);
                assert_eq!(request.export_name, None);
                assert!(request.fingerprint_roots.is_empty());
                3
            } else {
                assert_eq!(request.config_path, Some("runweaver.config.ts"));
                assert_eq!(request.export_name, Some("agentHooksConfig"));
                assert_eq!(request.fingerprint_roots, &["src".to_owned()]);
                7
            };
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(input_count),
            })
        };
        let mut bare = CapturedIo::new();
        let mut ok = CapturedIo::new();

        assert_eq!(
            run_runweaver_cli(
                &args(&["compile", "binary", "--cwd", root.to_str().unwrap()]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                bare.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            bare.stdout(),
            "Wrote .runweaver/bin/runweaver (3 inputs, sha256-fixture)\n"
        );

        assert_eq!(
            run_runweaver_cli(
                &args(&[
                    "compile",
                    "binary",
                    "--cwd",
                    root.to_str().unwrap(),
                    "--config",
                    "runweaver.config.ts",
                    "--export",
                    "agentHooksConfig",
                    "--fingerprint",
                    "src",
                ]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                ok.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            ok.stdout(),
            "Wrote .runweaver/bin/runweaver (7 inputs, sha256-fixture)\n"
        );
        assert_eq!(compile_calls.get(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runweaver_cli_help_and_unknown_command_match_public_text() {
        let root = temp_root("help");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let load_config = |_request: LoadRunweaverConfigRequest<'_>| Ok(config.clone());
        let load_hooks = |_request: LoadRunweaverAgentHooksConfigRequest<'_>| Ok(hooks.clone());
        let compile = |_request: CompileRunweaverBinaryRequest<'_>| {
            Ok(CompileRunweaverBinaryResult {
                outfile: root.join(".runweaver/bin/runweaver"),
                manifest: manifest(0),
            })
        };
        let mut help = CapturedIo::new();
        let mut unknown = CapturedIo::new();

        assert_eq!(
            run_runweaver_cli(
                &args(&["help"]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                help.io(""),
            )
            .unwrap(),
            0
        );
        assert!(help.stdout().starts_with("runweaver <command>"));

        assert_eq!(
            run_runweaver_cli(
                &args(&["wat", "--cwd", root.to_str().unwrap()]),
                RunweaverCliRuntime {
                    load_runweaver_config: &load_config,
                    load_agent_hooks_config: &load_hooks,
                    compile_binary: &compile,
                    generated_surface_files: &empty_generated_surface_files,
                    git_surface: &empty_git_surface,
                },
                unknown.io(""),
            )
            .unwrap(),
            1
        );
        assert!(
            unknown
                .stderr()
                .starts_with("Unknown runweaver command: wat")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
