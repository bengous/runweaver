use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::bindings::{Binding, bind};
use crate::core::OperationDefinition;
use crate::profiles::Profile;
use crate::surfaces::agent_hooks::{
    AgentHooksConfig, AgentHooksConfigHook, Harness, HarnessTarget, HookBinding, HookCommandSpec,
    define_hook,
};
use crate::surfaces::agent_hooks::{HookEvent, HookOutcome, HookStage};

use super::declarative::agent_hooks_config;
use super::validate::format_binding_issues;
use super::{
    ActionResult, ActionTask, ParallelTask, RunweaverDefinition, RunweaverOperationDefinition,
    SeriesTask, TaskCompletion, TaskDefinition, TaskOutput, normalize_file_path,
};

/// Manifest format version accepted by [`load_runweaver_manifest`]; other
/// versions are rejected with [`ManifestLoadError::UnsupportedVersion`].
pub const RUNWEAVER_DEFINITION_MANIFEST_VERSION: u32 = 2;

/// Project-specific binary identity used only while compiling sync surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunweaverProjectBinary {
    /// Cargo package that owns the project binary.
    pub package: String,
    /// Binary executable name.
    pub binary_name: String,
    /// Repo-relative compiled binary output path.
    pub out_path: String,
    /// Agent hooks config name.
    pub hooks_config_name: String,
    /// Loader-free fallback command prefix.
    pub fallback_command: String,
}

/// The serializable, closure-free form of a definition: path zones, tools,
/// pipelines, operations, surfaces, and bindings as pure data. Executable
/// behavior is referenced by builtin name and supplied at load time by a
/// [`BuiltinRegistry`]. The JSON Schema for this type is exported via
/// [`runweaver_manifest_json_schema`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunweaverDefinitionManifest {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<PathZonesManifest>,
    pub tools: BTreeMap<String, ToolDefinitionManifest>,
    pub pipelines: BTreeMap<String, PipelineDefinitionManifest>,
    pub operations: BTreeMap<String, RunweaverOperationDefinitionManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfaces: Option<SurfacesManifest>,
    pub bindings: Vec<BindingManifest>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(
    description = "Path zones used by Runweaver surfaces. Each entry is a repository-relative path. Entries ending with / are prefix zones; other entries are exact file paths. Leading ./ is ignored and path separators normalize to /. Absolute paths inside the current hook cwd are normalized back to repository-relative paths before matching."
)]
pub struct PathZonesManifest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub check_only: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_only: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ToolDefinitionManifest {
    Preset {
        preset: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        targets: Option<ToolTargetsManifest>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        affected: Vec<String>,
    },
    Declarative {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        targets: Option<ToolTargetsManifest>,
        check: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fix: Option<Vec<String>>,
        diagnostics: DiagnosticsParserManifest,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        affected: Vec<String>,
    },
    Script {
        script: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ToolTargetsManifest {
    Patterns(Vec<String>),
    Selectors(FileTargetsManifest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DiagnosticsParserManifest {
    Named {
        parser: NamedDiagnosticsParserManifest,
    },
    Regex {
        parser: NamedDiagnosticsParserManifest,
        pattern: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NamedDiagnosticsParserManifest {
    Unix,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PipelineDefinitionManifest {
    Check {
        check: Vec<String>,
    },
    Fix {
        fix: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        then: Option<Box<PipelineDefinitionManifest>>,
    },
    Stages {
        stages: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SurfacesManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<AgentsSurfaceManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitSurfaceManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<CiSurfaceManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentsSurfaceManifest {
    pub harnesses: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_tool: Vec<AgentsPreToolGuardManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_edit: Option<AgentsPipelineSlotManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<AgentsPipelineSlotManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AgentsPreToolGuardManifest {
    Builtin { guard: AgentsBuiltinGuardManifest },
    Tool { guard: String, tool: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AgentsBuiltinGuardManifest {
    DestructiveCommands,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentsPipelineSlotManifest {
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitSurfaceManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_commit: Option<GitPreCommitSlotManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_msg: Option<GitToolSlotManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_push: Option<GitPipelineSlotManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_commit: Option<GitToolSlotManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPreCommitSlotManifest {
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<GitFilesScopeManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GitFilesScopeManifest {
    Staged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitPipelineSlotManifest {
    pub run: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GitToolSlotManifest {
    pub tool: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CiSurfaceManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<GithubCiSurfaceManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GithubCiSurfaceManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileTargetsManifest {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefixes: Vec<String>,
    /// File arguments used when a file-scoped tool runs without explicit files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EmptyScopeManifest {
    Allow,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunweaverOperationDefinitionManifest {
    Operation {
        builtin: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentHooksConfigManifest {
    pub name: String,
    pub binary_name: String,
    pub source_path: String,
    pub harnesses: Vec<String>,
    pub targets: Vec<HarnessTargetManifest>,
    pub hooks: Vec<AgentHooksConfigHookManifest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HarnessTargetManifest {
    pub harness: String,
    pub path: String,
    pub command_prefix: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub options: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentHooksConfigHookManifest {
    pub command: HookCommandManifest,
    pub bindings: Vec<HookBindingManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HookCommandManifest {
    pub name: String,
    pub stage: crate::surfaces::agent_hooks::HookStage,
    pub builtin: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harnesses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HookBindingManifest {
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_prefix: Option<String>,
    pub timeout: u32,
    pub status_message: String,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub options: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BindingManifest {
    pub trigger: crate::surfaces::SurfaceTrigger,
    pub operation_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<ProfileManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileManifest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
    pub before_operation: bool,
    pub after_operation: bool,
    pub on_operation_error: bool,
}

/// Supplies the executable pieces a manifest references by name: operations,
/// profiles, harnesses, and hook commands. Names missing at load time fail
/// fast with [`ManifestLoadError::UnknownBuiltins`].
#[derive(Clone, Default)]
pub struct BuiltinRegistry {
    operations: HashMap<String, OperationDefinition>,
    profiles: HashMap<String, Profile>,
    harnesses: HashMap<String, Harness<'static>>,
    hook_commands: HashMap<String, crate::surfaces::agent_hooks::HookFn>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn operation(mut self, name: impl Into<String>, operation: OperationDefinition) -> Self {
        self.operations.insert(name.into(), operation);
        self
    }

    pub fn profile(mut self, name: impl Into<String>, profile: Profile) -> Self {
        self.profiles.insert(name.into(), profile);
        self
    }

    pub fn harness(mut self, name: impl Into<String>, harness: Harness<'static>) -> Self {
        self.harnesses.insert(name.into(), harness);
        self
    }

    pub fn hook_command(
        mut self,
        name: impl Into<String>,
        run: impl Fn(
            &crate::surfaces::agent_hooks::HookEvent,
        ) -> anyhow::Result<crate::surfaces::agent_hooks::HookOutcome>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.hook_commands
            .insert(name.into(), std::sync::Arc::new(run));
        self
    }
}

/// The registry shipped with the generic `runweaver` binary: the built-in
/// `claude` and `codex` harnesses plus the `guardDestructive` hook command.
/// Manifests that reference anything else need a project-specific binary.
pub fn default_builtin_registry() -> BuiltinRegistry {
    BuiltinRegistry::new()
        .harness(
            crate::surfaces::agent_hooks::BuiltInHarnessName::Claude.as_str(),
            crate::surfaces::agent_hooks::claude_harness(),
        )
        .harness(
            crate::surfaces::agent_hooks::BuiltInHarnessName::Codex.as_str(),
            crate::surfaces::agent_hooks::codex_harness(),
        )
        .hook_command(
            "guardDestructive",
            crate::surfaces::agent_hooks::guard_destructive,
        )
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("Unsupported Runweaver manifest version {actual}; expected {expected}.")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("Runweaver manifest references unknown builtins:\n{0}")]
    UnknownBuiltins(String),
    #[error("Invalid Runweaver manifest definition:\n{0}")]
    InvalidDefinition(String),
    #[error("Invalid Runweaver manifest hook config: {0}")]
    InvalidAgentHooksConfig(String),
    #[error("Failed to serialize Runweaver manifest schema: {0}")]
    SchemaSerialize(String),
    #[error("Failed to write Runweaver manifest artifact {path}: {message}")]
    Io { path: String, message: String },
}

/// Everything a manifest load produces: the executable definition, optional
/// agent-hook config, generated surface files, and the surfaces manifest.
#[derive(Debug, Clone)]
pub struct LoadedRunweaverManifest {
    pub definition: RunweaverDefinition,
    pub agent_hooks: Option<AgentHooksConfig<'static>>,
    pub generated_surface_files: Vec<GeneratedSurfaceFile>,
    pub surfaces: Option<SurfacesManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSurfaceFile {
    pub path: String,
    pub content: String,
    pub executable: bool,
}

/// Projects a definition into its pure-data manifest shape (closures are
/// represented by name/presence only, never serialized).
pub fn create_runweaver_definition_manifest(
    definition: &RunweaverDefinition,
) -> RunweaverDefinitionManifest {
    RunweaverDefinitionManifest {
        version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
        paths: None,
        tools: BTreeMap::new(),
        pipelines: definition
            .tasks
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    PipelineDefinitionManifest::Check { check: Vec::new() },
                )
            })
            .collect(),
        operations: definition
            .operations
            .iter()
            .map(|(name, operation)| (name.clone(), operation_manifest(operation)))
            .collect(),
        surfaces: None,
        bindings: definition.bindings.iter().map(binding_manifest).collect(),
    }
}

/// Like [`load_runweaver_manifest`] but returns only the executable
/// definition.
pub fn load_runweaver_definition(
    manifest: &RunweaverDefinitionManifest,
    registry: &BuiltinRegistry,
    project_binary: &RunweaverProjectBinary,
) -> Result<RunweaverDefinition, ManifestLoadError> {
    Ok(load_runweaver_manifest(manifest, registry, project_binary)?.definition)
}

/// Turns a manifest plus a [`BuiltinRegistry`] into a
/// [`LoadedRunweaverManifest`]. Fails fast on version mismatch, unknown
/// builtins (grouped into one error), or an invalid resulting definition.
pub fn load_runweaver_manifest(
    manifest: &RunweaverDefinitionManifest,
    registry: &BuiltinRegistry,
    project_binary: &RunweaverProjectBinary,
) -> Result<LoadedRunweaverManifest, ManifestLoadError> {
    if manifest.version != RUNWEAVER_DEFINITION_MANIFEST_VERSION {
        return Err(ManifestLoadError::UnsupportedVersion {
            actual: manifest.version,
            expected: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
        });
    }

    let mut missing = MissingBuiltins::default();
    let mut definition = RunweaverDefinition::new();
    load_tool_contract_tasks(manifest, &mut definition);
    for (name, operation) in &manifest.operations {
        if let Some(loaded) = load_operation(operation, registry, &mut missing) {
            definition.operations.insert(name.clone(), loaded);
        }
    }
    for binding in &manifest.bindings {
        if let Some(loaded) = load_binding(binding, registry, &mut missing) {
            definition.bindings.push(loaded);
        }
    }
    let agent_hooks_manifest = manifest
        .surfaces
        .as_ref()
        .and_then(|surfaces| surfaces.agents.as_ref())
        .map(|agents| {
            agents_surface_to_agent_hooks_manifest(
                agents,
                manifest.paths.as_ref(),
                &definition.task_config(),
                project_binary,
            )
        });
    let agent_hooks = agent_hooks_manifest.as_ref().and_then(|hooks| {
        load_agent_hooks(
            hooks,
            manifest.paths.as_ref(),
            &definition.task_config(),
            registry,
            &mut missing,
        )
    });
    let generated_surface_files = generated_surface_files(manifest, project_binary);

    missing.into_result()?;

    let validation = definition.validate();
    if crate::diagnostics::has_error_diagnostics(&validation.config_diagnostics) {
        return Err(ManifestLoadError::InvalidDefinition(
            crate::diagnostics::format_diagnostics(&validation.config_diagnostics),
        ));
    }
    if !validation.binding_validation.ok {
        return Err(ManifestLoadError::InvalidDefinition(format_binding_issues(
            &validation.binding_validation.issues,
        )));
    }

    Ok(LoadedRunweaverManifest {
        definition,
        agent_hooks: match agent_hooks {
            Some(Ok(config)) => Some(config),
            Some(Err(error)) => {
                return Err(ManifestLoadError::InvalidAgentHooksConfig(
                    error.to_string(),
                ));
            }
            None => None,
        },
        generated_surface_files,
        surfaces: manifest.surfaces.clone(),
    })
}

pub fn runweaver_manifest_json_schema() -> serde_json::Value {
    serde_json::to_value(schema_for!(RunweaverDefinitionManifest))
        .unwrap_or_else(|_| serde_json::json!({}))
}

pub fn runweaver_manifest_schema_content() -> Result<String, ManifestLoadError> {
    serde_json::to_string_pretty(&runweaver_manifest_json_schema())
        .map(|content| content + "\n")
        .map_err(|error| ManifestLoadError::SchemaSerialize(error.to_string()))
}

pub fn runweaver_manifest_schema_sha256() -> String {
    use sha2::{Digest, Sha256};
    let content = runweaver_manifest_schema_content().unwrap_or_default();
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub fn write_runweaver_manifest_schema(path: &Path) -> Result<(), ManifestLoadError> {
    let content = runweaver_manifest_schema_content()?;
    std::fs::write(path, content).map_err(|error| ManifestLoadError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

pub fn write_runweaver_manifest(
    path: &Path,
    manifest: &RunweaverDefinitionManifest,
) -> Result<(), ManifestLoadError> {
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|error| ManifestLoadError::SchemaSerialize(error.to_string()))?;
    std::fs::write(path, content + "\n").map_err(|error| ManifestLoadError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolRunMode {
    Check,
    Fix,
}

#[derive(Debug, Clone)]
struct ExecutableToolSpec {
    program: String,
    base_args: Vec<String>,
    check_args: Vec<String>,
    fix_args: Option<Vec<String>>,
    targets: ToolTargets,
    affected: Vec<String>,
    parser: Option<DiagnosticsParserManifest>,
    script: bool,
    whole_program: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ToolTargets {
    extensions: Vec<String>,
    files: Vec<String>,
    prefixes: Vec<String>,
    fallback: Vec<String>,
}

fn load_tool_contract_tasks(
    manifest: &RunweaverDefinitionManifest,
    definition: &mut RunweaverDefinition,
) {
    let path_zones = manifest.paths.clone().unwrap_or_default();
    let tools = manifest
        .tools
        .iter()
        .map(|(name, tool)| (name.clone(), executable_tool(name, tool)))
        .collect::<BTreeMap<_, _>>();

    for (pipeline_name, pipeline) in &manifest.pipelines {
        compile_pipeline(
            pipeline_name,
            pipeline,
            &tools,
            &path_zones,
            &mut definition.tasks,
        );
    }
    for (tool_name, tool) in tools {
        if definition.tasks.contains_key(&tool_name) {
            continue;
        }
        let zones = path_zones.clone();
        definition.tasks.insert(
            tool_name,
            TaskDefinition::Action(ActionTask::new(move |ctx| {
                run_manifest_tool(&tool, ToolRunMode::Check, &zones, ctx)
            })),
        );
    }
}

fn compile_pipeline(
    name: &str,
    pipeline: &PipelineDefinitionManifest,
    tools: &BTreeMap<String, ExecutableToolSpec>,
    path_zones: &PathZonesManifest,
    tasks: &mut HashMap<String, TaskDefinition>,
) {
    let task = match pipeline {
        PipelineDefinitionManifest::Check { check } => {
            insert_tool_tasks(name, check, ToolRunMode::Check, tools, path_zones, tasks);
            TaskDefinition::Parallel(ParallelTask {
                refs: check
                    .iter()
                    .map(|tool_name| tool_task_name(name, tool_name, ToolRunMode::Check))
                    .collect(),
                fail_fast: false,
                policies: Vec::new(),
            })
        }
        PipelineDefinitionManifest::Fix { fix, then } => {
            insert_tool_tasks(name, fix, ToolRunMode::Fix, tools, path_zones, tasks);
            let mut refs = fix
                .iter()
                .map(|tool_name| tool_task_name(name, tool_name, ToolRunMode::Fix))
                .collect::<Vec<_>>();
            if let Some(then) = then {
                let then_name = format!("__pipeline:{name}:then");
                compile_pipeline(&then_name, then, tools, path_zones, tasks);
                refs.push(then_name);
            }
            TaskDefinition::Series(SeriesTask {
                refs,
                fail_fast: true,
                policies: Vec::new(),
            })
        }
        PipelineDefinitionManifest::Stages { stages } => TaskDefinition::Series(SeriesTask {
            refs: stages.clone(),
            fail_fast: false,
            policies: Vec::new(),
        }),
    };
    tasks.insert(name.to_owned(), task);
}

fn tool_task_name(pipeline: &str, tool: &str, mode: ToolRunMode) -> String {
    let mode = match mode {
        ToolRunMode::Check => "check",
        ToolRunMode::Fix => "fix",
    };
    format!("__tool:{pipeline}:{tool}:{mode}")
}

fn insert_tool_tasks(
    pipeline: &str,
    tool_names: &[String],
    mode: ToolRunMode,
    tools: &BTreeMap<String, ExecutableToolSpec>,
    path_zones: &PathZonesManifest,
    tasks: &mut HashMap<String, TaskDefinition>,
) {
    for tool_name in tool_names {
        let Some(tool) = tools.get(tool_name).cloned() else {
            continue;
        };
        let task_name = tool_task_name(pipeline, tool_name, mode);
        let zones = path_zones.clone();
        tasks.insert(
            task_name,
            TaskDefinition::Action(ActionTask::new(move |ctx| {
                run_manifest_tool(&tool, mode, &zones, ctx)
            })),
        );
    }
}

fn run_manifest_tool(
    tool: &ExecutableToolSpec,
    requested_mode: ToolRunMode,
    path_zones: &PathZonesManifest,
    ctx: &super::ExecutionContext,
) -> ActionResult {
    let resolved_mode = if requested_mode == ToolRunMode::Fix
        && !ctx.files.is_empty()
        && scoped_files(ctx, &path_zones.check_only).is_empty()
    {
        ToolRunMode::Fix
    } else if requested_mode == ToolRunMode::Fix && !ctx.files.is_empty() {
        ToolRunMode::Check
    } else {
        requested_mode
    };
    let Some(args) = manifest_tool_args(tool, resolved_mode, path_zones, ctx) else {
        return ActionResult::skipped_with_reason("No matching target files.");
    };
    let output = spawn_manifest_tool(&tool.program, &args, ctx);
    let completion = if output.exit_code == Some(0) {
        TaskCompletion::Success
    } else if output.exit_code.is_some() {
        TaskCompletion::Error
    } else {
        TaskCompletion::ToolError
    };
    let mut data = serde_json::Map::new();
    let diagnostics = parse_diagnostics(tool.parser.as_ref(), &output);
    if !diagnostics.is_empty() {
        data.insert("diagnostics".to_owned(), serde_json::json!(diagnostics));
    }
    ActionResult::completed()
        .completion(completion)
        .output(output)
        .data(serde_json::Value::Object(data))
        .build()
}

fn manifest_tool_args(
    tool: &ExecutableToolSpec,
    mode: ToolRunMode,
    path_zones: &PathZonesManifest,
    ctx: &super::ExecutionContext,
) -> Option<Vec<String>> {
    if tool.script {
        if mode == ToolRunMode::Fix {
            return None;
        }
        let mut args = tool.base_args.clone();
        append_input_args(&mut args, ctx);
        return Some(args);
    }
    let mut args = tool.base_args.clone();
    let mode_args = match mode {
        ToolRunMode::Check => tool.check_args.clone(),
        ToolRunMode::Fix => tool
            .fix_args
            .clone()
            .unwrap_or_else(|| tool.check_args.clone()),
    };
    let files = resolved_files(tool, mode, path_zones, ctx);
    if tool.program == "cargo"
        && matches!(mode, ToolRunMode::Fix)
        && tool.fix_args == Some(vec!["fmt".to_owned()])
    {
        if !ctx.files.is_empty() && files.is_empty() {
            return None;
        }
        args.extend(cargo_fmt_fix_args(&files, ctx));
        append_input_args(&mut args, ctx);
        return Some(args);
    }
    if !tool.whole_program && ctx.files.is_empty() && !tool.targets.fallback.is_empty() {
        replace_files_placeholder_or_append(&mut args, mode_args, &tool.targets.fallback);
        append_input_args(&mut args, ctx);
        return Some(args);
    }
    if !tool.whole_program && !ctx.files.is_empty() && files.is_empty() {
        return None;
    }
    let mut scoped = if files.is_empty() {
        tool.targets.fallback.clone()
    } else {
        files
    };
    // `bun test <relative-path>` treats bare arguments as name filters, not
    // paths; a `./` prefix is required for path-based filtering.
    if tool.program == "bun" && tool.check_args.first().map(String::as_str) == Some("test") {
        for file in &mut scoped {
            if !file.starts_with('/') && !file.starts_with("./") {
                *file = format!("./{file}");
            }
        }
    }
    replace_files_placeholder_or_append(&mut args, mode_args, &scoped);
    if !tool.whole_program || tool.program == "commitlint" {
        append_input_args(&mut args, ctx);
    }
    Some(args)
}

fn append_input_args(args: &mut Vec<String>, ctx: &super::ExecutionContext) {
    let Some(input_args) = ctx
        .input
        .as_ref()
        .and_then(|input| input.get("args"))
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    args.extend(
        input_args
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    );
}

fn cargo_fmt_fix_args(files: &[String], ctx: &super::ExecutionContext) -> Vec<String> {
    let mut args = vec!["fmt".to_owned()];
    if ctx.files.is_empty() {
        args.push("--all".to_owned());
        return args;
    }
    let mut packages = BTreeSet::new();
    for file in files {
        if let Some(rest) = file.strip_prefix("crates/")
            && let Some((package, _)) = rest.split_once('/')
            && !package.is_empty()
        {
            packages.insert(package.to_owned());
        }
    }
    for package in packages {
        args.push("-p".to_owned());
        args.push(package);
    }
    args
}

fn replace_files_placeholder_or_append(
    args: &mut Vec<String>,
    mut mode_args: Vec<String>,
    files: &[String],
) {
    let mut replaced = false;
    for arg in &mut mode_args {
        if arg == "{files}" {
            *arg = files.join(" ");
            replaced = true;
        }
    }
    args.extend(mode_args);
    if !replaced {
        args.extend(files.iter().cloned());
    }
}

fn resolved_files(
    tool: &ExecutableToolSpec,
    mode: ToolRunMode,
    path_zones: &PathZonesManifest,
    ctx: &super::ExecutionContext,
) -> Vec<String> {
    if ctx.files.is_empty() || tool.whole_program {
        return Vec::new();
    }
    let mut files = if mode == ToolRunMode::Fix {
        scoped_files(ctx, &path_zones.writable)
    } else {
        ctx.files
            .iter()
            .map(|file| normalize_file_path(file))
            .collect()
    };
    files.retain(|file| matches_tool_targets(&tool.targets, file));
    let mut affected = Vec::new();
    for file in &files {
        affected.extend(resolve_affected(&tool.affected, file));
    }
    files.extend(affected);
    files.sort();
    files.dedup();
    files
}

fn scoped_files(ctx: &super::ExecutionContext, zones: &[String]) -> Vec<String> {
    ctx.files
        .iter()
        .map(|file| normalize_file_path(file))
        .filter(|file| zones.iter().any(|zone| path_in_zone(file, zone)))
        .collect()
}

fn path_in_zone(file: &str, zone: &str) -> bool {
    let zone = normalize_file_path(zone);
    if zone.ends_with('/') {
        file.starts_with(&zone)
    } else {
        file == zone
    }
}

fn matches_tool_targets(targets: &ToolTargets, file: &str) -> bool {
    let extension_matches = targets.extensions.is_empty()
        || targets
            .extensions
            .iter()
            .any(|extension| file.ends_with(&format!(".{}", extension.trim_start_matches('.'))));
    let path_matches = targets.files.is_empty() && targets.prefixes.is_empty()
        || targets
            .files
            .iter()
            .any(|target| normalize_file_path(target) == file)
        || targets
            .prefixes
            .iter()
            .any(|prefix| path_in_zone(file, prefix));
    extension_matches && path_matches
}

fn resolve_affected(patterns: &[String], file: &str) -> Vec<String> {
    let path = Path::new(file);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let dir = path.parent().and_then(|dir| dir.to_str()).unwrap_or("");
    patterns
        .iter()
        .map(|pattern| pattern.replace("{stem}", stem).replace("{dir}", dir))
        .collect()
}

fn spawn_manifest_tool(
    program: &str,
    args: &[String],
    ctx: &super::ExecutionContext,
) -> TaskOutput {
    let executable = resolve_manifest_executable(program, ctx);
    let mut command = Command::new(&executable);
    command.args(args).current_dir(&ctx.cwd).env_clear();
    for (key, value) in &ctx.env {
        command.env(key, value);
    }
    command.env(
        "PATH",
        crate::toolchain::managed_tool_path_env(
            Path::new(&ctx.cwd),
            ctx.env.get("PATH").map(String::as_str),
        ),
    );
    match command.output() {
        Ok(output) => TaskOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: None,
        },
        Err(error) => TaskOutput::tool_error(format!("failed to spawn {program}: {error}")),
    }
}

fn resolve_manifest_executable(program: &str, ctx: &super::ExecutionContext) -> PathBuf {
    match crate::toolchain::resolve_managed_binary(Path::new(&ctx.cwd), program) {
        crate::toolchain::ResolveManagedBinaryResult::Found { path } => path,
        crate::toolchain::ResolveManagedBinaryResult::Missing { .. } => PathBuf::from(program),
    }
}

fn parse_diagnostics(
    parser: Option<&DiagnosticsParserManifest>,
    output: &TaskOutput,
) -> Vec<serde_json::Value> {
    let Some(parser) = parser else {
        return Vec::new();
    };
    let text = format!("{}{}", output.stdout, output.stderr);
    match parser {
        DiagnosticsParserManifest::Named {
            parser: NamedDiagnosticsParserManifest::Unix,
        } => parse_regex_diagnostics(
            r"^(?P<file>.*?):(?P<line>\d+):(?P<col>\d+):\s*(?P<message>.*)$",
            &text,
        ),
        DiagnosticsParserManifest::Regex { pattern, .. } => parse_regex_diagnostics(pattern, &text),
        DiagnosticsParserManifest::Named {
            parser: NamedDiagnosticsParserManifest::Regex,
        } => Vec::new(),
    }
}

fn parse_regex_diagnostics(pattern: &str, text: &str) -> Vec<serde_json::Value> {
    let Ok(regex) = regex::Regex::new(pattern) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| regex.captures(line))
        .map(|captures| {
            let mut diagnostic = serde_json::Map::new();
            for field in ["file", "line", "col", "code", "message"] {
                if let Some(value) = captures.name(field) {
                    diagnostic.insert(field.to_owned(), serde_json::json!(value.as_str()));
                }
            }
            serde_json::Value::Object(diagnostic)
        })
        .collect()
}

fn agents_surface_to_agent_hooks_manifest(
    agents: &AgentsSurfaceManifest,
    _paths: Option<&PathZonesManifest>,
    _config: &super::RunweaverConfig,
    project_binary: &RunweaverProjectBinary,
) -> AgentHooksConfigManifest {
    let pi_prefix = agent_hook_command_prefix(
        "pi",
        crate::surfaces::agent_hooks::RunweaverHookCommandCwd::Env("PI_PROJECT_DIR".to_owned()),
        &project_binary.out_path,
    );
    let codex_prefix = agent_hook_command_prefix(
        "codex",
        crate::surfaces::agent_hooks::RunweaverHookCommandCwd::GitRoot,
        &project_binary.out_path,
    );
    let claude_prefix = agent_hook_command_prefix(
        "claude",
        crate::surfaces::agent_hooks::RunweaverHookCommandCwd::Env("CLAUDE_PROJECT_DIR".to_owned()),
        &project_binary.out_path,
    );
    let mut hooks = Vec::new();

    for guard in &agents.pre_tool {
        match guard {
            AgentsPreToolGuardManifest::Builtin {
                guard: AgentsBuiltinGuardManifest::DestructiveCommands,
            } => hooks.push(hook(
                "guard-destructive",
                HookStage::PreTool,
                "guardDestructive",
                agents
                    .harnesses
                    .iter()
                    .map(|harness| destructive_binding(harness))
                    .collect(),
            )),
            AgentsPreToolGuardManifest::Tool { .. } => {}
        }
    }

    if let Some(slot) = &agents.post_edit {
        hooks.push(hook(
            "post-edit-quality",
            HookStage::PostEdit,
            &format!("agentsPostEdit:{}", slot.run),
            agents
                .harnesses
                .iter()
                .map(|harness| post_edit_binding(harness, slot.timeout.unwrap_or(90)))
                .collect(),
        ));
    }

    if let Some(slot) = &agents.stop {
        hooks.push(hook(
            "stop-validate",
            HookStage::Stop,
            &format!("agentsStop:{}", slot.run),
            agents
                .harnesses
                .iter()
                .map(|harness| stop_binding(harness, slot.timeout.unwrap_or(240)))
                .collect(),
        ));
    }

    AgentHooksConfigManifest {
        name: project_binary.hooks_config_name.clone(),
        binary_name: format!("{} hook", project_binary.binary_name),
        source_path: "runweaver.config.ts".to_owned(),
        harnesses: agents.harnesses.clone(),
        targets: agents
            .harnesses
            .iter()
            .filter_map(|harness| match harness.as_str() {
                "pi" => Some(target("pi", ".pi/hooks.jsonc", &pi_prefix, [])),
                "codex" => Some(target(
                    "codex",
                    ".codex/config.toml",
                    &codex_prefix,
                    [("features", serde_json::json!({ "hooks": true }))],
                )),
                "claude" => Some(target(
                    "claude",
                    ".claude/settings.json",
                    &claude_prefix,
                    [(
                        "worktreeSymlinkDirectories",
                        serde_json::json!(["node_modules"]),
                    )],
                )),
                _ => None,
            })
            .collect(),
        hooks,
    }
}

fn target(
    harness: &str,
    path: &str,
    command_prefix: &str,
    options: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> HarnessTargetManifest {
    HarnessTargetManifest {
        harness: harness.to_owned(),
        path: path.to_owned(),
        command_prefix: command_prefix.to_owned(),
        options: options
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    }
}

fn hook(
    name: &str,
    stage: HookStage,
    builtin: &str,
    bindings: Vec<HookBindingManifest>,
) -> AgentHooksConfigHookManifest {
    AgentHooksConfigHookManifest {
        command: HookCommandManifest {
            name: name.to_owned(),
            stage,
            builtin: builtin.to_owned(),
            harnesses: Vec::new(),
        },
        bindings,
    }
}

fn destructive_binding(harness: &str) -> HookBindingManifest {
    pre_tool_binding(
        harness,
        10,
        match harness {
            "claude" => "Checking for destructive commands...",
            _ => "Checking destructive commands",
        },
    )
}

fn pre_tool_binding(harness: &str, timeout: u32, status_message: &str) -> HookBindingManifest {
    binding(
        harness,
        timeout,
        status_message,
        match harness {
            "codex" => Some("^Bash$"),
            _ => Some("Bash"),
        },
    )
}

fn post_edit_binding(harness: &str, timeout: u32) -> HookBindingManifest {
    binding(
        harness,
        timeout,
        match harness {
            "claude" => "Formatting and linting...",
            _ => "Formatting and linting edited files",
        },
        match harness {
            "codex" => Some("^(apply_patch|Edit|Write|MultiEdit)$"),
            "claude" => Some("Edit|Write|MultiEdit"),
            _ => Some("Edit|Write"),
        },
    )
}

fn stop_binding(harness: &str, timeout: u32) -> HookBindingManifest {
    binding(
        harness,
        timeout,
        match harness {
            "pi" => "Running Pi validation",
            "codex" => "Running Codex validation",
            "claude" => "Scope-aware validation...",
            _ => "Running validation",
        },
        None,
    )
}

fn binding(
    harness: &str,
    timeout: u32,
    status_message: &str,
    matcher: Option<&str>,
) -> HookBindingManifest {
    HookBindingManifest {
        harness: harness.to_owned(),
        matcher: matcher.map(str::to_owned),
        command_prefix: None,
        timeout,
        status_message: status_message.to_owned(),
        options: serde_json::Map::new(),
    }
}

fn agent_hook_command_prefix(
    harness: &str,
    cwd: crate::surfaces::agent_hooks::RunweaverHookCommandCwd,
    out_path: &str,
) -> String {
    crate::surfaces::agent_hooks::compiled_runweaver_hook_command(
        &crate::surfaces::agent_hooks::CompiledRunweaverHookCommandOptions::new(
            harness,
            cwd,
            format!("./{out_path}"),
        ),
    )
    .unwrap_or_else(|error| panic!("Invalid compiled hook command prefix: {error}"))
}

fn executable_tool(name: &str, tool: &ToolDefinitionManifest) -> ExecutableToolSpec {
    match tool {
        ToolDefinitionManifest::Preset {
            preset,
            args,
            targets,
            affected,
        } => preset_tool(preset, args, targets.as_ref(), affected),
        ToolDefinitionManifest::Declarative {
            targets,
            check,
            fix,
            diagnostics,
            affected,
        } => {
            let mut check_parts = check.clone();
            let program = if check_parts.is_empty() {
                name.to_owned()
            } else {
                check_parts.remove(0)
            };
            let fix_args = fix.as_ref().map(|fix| {
                let mut args = fix.clone();
                if args.first().is_some_and(|part| part == &program) {
                    args.remove(0);
                }
                args
            });
            ExecutableToolSpec {
                program,
                base_args: Vec::new(),
                check_args: check_parts,
                fix_args,
                targets: manifest_targets(targets.as_ref()),
                affected: affected.clone(),
                parser: Some(diagnostics.clone()),
                script: false,
                whole_program: false,
            }
        }
        ToolDefinitionManifest::Script { script } => ExecutableToolSpec {
            program: "/bin/sh".to_owned(),
            base_args: vec!["-c".to_owned(), script.clone()],
            check_args: Vec::new(),
            fix_args: None,
            targets: ToolTargets::default(),
            affected: Vec::new(),
            parser: None,
            script: true,
            whole_program: true,
        },
    }
}

fn preset_tool(
    preset: &str,
    args: &[String],
    targets: Option<&ToolTargetsManifest>,
    affected: &[String],
) -> ExecutableToolSpec {
    match preset {
        "oxfmt" => ExecutableToolSpec {
            program: "oxfmt".to_owned(),
            base_args: args.to_vec(),
            check_args: vec!["--check".to_owned()],
            fix_args: Some(vec!["--write".to_owned()]),
            targets: preset_targets(&["ts", "tsx", "js", "jsx", "mjs", "json", "jsonc"], targets),
            affected: affected.to_vec(),
            parser: None,
            script: false,
            whole_program: false,
        },
        "oxlint" => ExecutableToolSpec {
            program: "oxlint".to_owned(),
            base_args: args.to_vec(),
            check_args: vec!["--quiet".to_owned(), "--format=unix".to_owned()],
            fix_args: Some(vec!["--fix".to_owned()]),
            targets: preset_targets(&["ts", "tsx", "js", "jsx", "mjs"], targets),
            affected: affected.to_vec(),
            parser: Some(DiagnosticsParserManifest::Named {
                parser: NamedDiagnosticsParserManifest::Unix,
            }),
            script: false,
            whole_program: false,
        },
        "tsc" => ExecutableToolSpec {
            program: "tsc".to_owned(),
            base_args: args.to_vec(),
            check_args: vec![
                "--noEmit".to_owned(),
                "--pretty".to_owned(),
                "false".to_owned(),
            ],
            fix_args: None,
            targets: ToolTargets::default(),
            affected: Vec::new(),
            parser: None,
            script: false,
            whole_program: true,
        },
        "cargo-fmt" => ExecutableToolSpec {
            program: "cargo".to_owned(),
            base_args: Vec::new(),
            check_args: vec![
                "fmt".to_owned(),
                "--all".to_owned(),
                "--".to_owned(),
                "--check".to_owned(),
            ],
            fix_args: Some(vec!["fmt".to_owned()]),
            targets: preset_targets(&["rs"], targets),
            affected: affected.to_vec(),
            parser: None,
            script: false,
            whole_program: false,
        },
        "cargo-check" => cargo_preset("check", args, targets),
        "cargo-clippy" => cargo_preset("clippy", args, targets),
        "bun-test" => ExecutableToolSpec {
            program: "bun".to_owned(),
            base_args: Vec::new(),
            check_args: {
                let mut out = vec!["test".to_owned()];
                out.extend(args.iter().cloned());
                out
            },
            fix_args: None,
            targets: manifest_targets(targets),
            affected: affected.to_vec(),
            parser: None,
            script: false,
            whole_program: false,
        },
        "dependency-cruiser" => managed_check_preset("dependency-cruiser", args, targets),
        "knip" => managed_check_preset("knip", args, targets),
        "jscpd" => managed_check_preset("jscpd", args, targets),
        "gitleaks" => ExecutableToolSpec {
            program: "gitleaks".to_owned(),
            base_args: Vec::new(),
            check_args: args.to_vec(),
            fix_args: None,
            targets: ToolTargets::default(),
            affected: Vec::new(),
            parser: None,
            script: false,
            whole_program: true,
        },
        "commitlint" => ExecutableToolSpec {
            program: "commitlint".to_owned(),
            base_args: Vec::new(),
            check_args: args.to_vec(),
            fix_args: None,
            targets: ToolTargets::default(),
            affected: Vec::new(),
            parser: None,
            script: false,
            whole_program: true,
        },
        other => ExecutableToolSpec {
            program: other.to_owned(),
            base_args: Vec::new(),
            check_args: args.to_vec(),
            fix_args: None,
            targets: manifest_targets(targets),
            affected: affected.to_vec(),
            parser: None,
            script: false,
            whole_program: false,
        },
    }
}

/// Resolve manifest targets and apply preset extensions only when omitted.
fn preset_targets(
    default_extensions: &[&str],
    targets: Option<&ToolTargetsManifest>,
) -> ToolTargets {
    let mut resolved = manifest_targets(targets);
    if resolved.extensions.is_empty() {
        resolved.extensions = default_extensions
            .iter()
            .map(|extension| (*extension).to_owned())
            .collect();
    }
    resolved
}

fn cargo_preset(
    subcommand: &str,
    args: &[String],
    targets: Option<&ToolTargetsManifest>,
) -> ExecutableToolSpec {
    let mut check_args = vec![subcommand.to_owned()];
    check_args.extend(args.iter().cloned());
    ExecutableToolSpec {
        program: "cargo".to_owned(),
        base_args: Vec::new(),
        check_args,
        fix_args: None,
        targets: preset_targets(&["rs", "toml", "lock"], targets),
        affected: Vec::new(),
        parser: None,
        script: false,
        whole_program: true,
    }
}

fn managed_check_preset(
    program: &str,
    args: &[String],
    targets: Option<&ToolTargetsManifest>,
) -> ExecutableToolSpec {
    ExecutableToolSpec {
        program: program.to_owned(),
        base_args: Vec::new(),
        check_args: args.to_vec(),
        fix_args: None,
        targets: manifest_targets(targets),
        affected: Vec::new(),
        parser: None,
        script: false,
        whole_program: targets.is_none(),
    }
}

fn manifest_targets(targets: Option<&ToolTargetsManifest>) -> ToolTargets {
    match targets {
        Some(ToolTargetsManifest::Patterns(patterns)) => ToolTargets {
            fallback: patterns.clone(),
            files: patterns
                .iter()
                .filter(|pattern| !pattern.contains('*') && !pattern.ends_with('/'))
                .cloned()
                .collect(),
            prefixes: patterns
                .iter()
                .filter(|pattern| !pattern.contains('*'))
                .map(|pattern| {
                    let normalized = normalize_file_path(pattern);
                    if normalized.ends_with('/') {
                        normalized
                    } else {
                        format!("{normalized}/")
                    }
                })
                .collect(),
            ..ToolTargets::default()
        },
        Some(ToolTargetsManifest::Selectors(selectors)) => ToolTargets {
            extensions: selectors.extensions.clone(),
            files: selectors.files.clone(),
            prefixes: selectors.prefixes.clone(),
            fallback: selectors.fallback.clone(),
        },
        None => ToolTargets::default(),
    }
}

fn operation_manifest(
    operation: &RunweaverOperationDefinition,
) -> RunweaverOperationDefinitionManifest {
    match operation {
        RunweaverOperationDefinition::Operation(operation) => {
            RunweaverOperationDefinitionManifest::Operation {
                builtin: "<closure>".to_owned(),
                description: operation.description.clone(),
            }
        }
        RunweaverOperationDefinition::Task(_) => RunweaverOperationDefinitionManifest::Operation {
            builtin: "<task-operation-not-serializable>".to_owned(),
            description: None,
        },
    }
}

fn load_operation(
    operation: &RunweaverOperationDefinitionManifest,
    registry: &BuiltinRegistry,
    missing: &mut MissingBuiltins,
) -> Option<RunweaverOperationDefinition> {
    match operation {
        RunweaverOperationDefinitionManifest::Operation {
            builtin,
            description,
        } => {
            let mut operation = registry
                .operations
                .get(builtin)
                .cloned()
                .or_else(|| missing.record("operation", builtin))?;
            if let Some(description) = description {
                operation.description = Some(description.clone());
            }
            Some(RunweaverOperationDefinition::Operation(operation))
        }
    }
}

fn binding_manifest(binding: &Binding) -> BindingManifest {
    BindingManifest {
        trigger: binding.trigger.clone(),
        operation_name: binding.operation_name.clone(),
        profiles: binding
            .profiles
            .iter()
            .map(|profile| ProfileManifest {
                name: profile.name.clone(),
                builtin: None,
                before_operation: profile.before_operation.is_some(),
                after_operation: profile.after_operation.is_some(),
                on_operation_error: profile.on_operation_error.is_some(),
            })
            .collect(),
    }
}

fn load_binding(
    binding: &BindingManifest,
    registry: &BuiltinRegistry,
    missing: &mut MissingBuiltins,
) -> Option<Binding> {
    let mut profiles = Vec::new();
    for profile in &binding.profiles {
        let builtin = profile.builtin.as_deref().unwrap_or(&profile.name);
        if let Some(loaded) = registry.profiles.get(builtin) {
            profiles.push(loaded.clone());
        } else {
            missing.record::<Profile>("profile", builtin);
        }
    }
    if !binding.profiles.is_empty() && profiles.len() != binding.profiles.len() {
        return None;
    }
    Some(
        bind(binding.trigger.clone())
            .to(binding.operation_name.clone())
            .r#use(profiles),
    )
}

fn load_agent_hooks(
    manifest: &AgentHooksConfigManifest,
    paths: Option<&PathZonesManifest>,
    config: &super::RunweaverConfig,
    registry: &BuiltinRegistry,
    missing: &mut MissingBuiltins,
) -> Option<Result<AgentHooksConfig<'static>, crate::surfaces::agent_hooks::AgentHooksConfigError>>
{
    let mut builder = agent_hooks_config(
        manifest.name.clone(),
        manifest.binary_name.clone(),
        manifest.source_path.clone(),
    );
    for harness in &manifest.harnesses {
        if let Some(loaded) = registry.harnesses.get(harness) {
            builder.harness(loaded.clone());
        } else {
            missing.record::<Harness<'static>>("harness", harness);
        }
    }
    for target in &manifest.targets {
        builder.target(HarnessTarget {
            harness: target.harness.clone(),
            path: target.path.clone(),
            command_prefix: target.command_prefix.clone(),
            options: target.options.clone(),
        });
    }
    for hook in &manifest.hooks {
        let Some(command) = load_hook_command(&hook.command, config, registry, missing) else {
            continue;
        };
        builder.hook(
            define_hook(AgentHooksConfigHook {
                command,
                bindings: hook
                    .bindings
                    .iter()
                    .map(|binding| HookBinding {
                        harness: binding.harness.clone(),
                        matcher: binding.matcher.clone(),
                        command_prefix: binding.command_prefix.clone(),
                        timeout: binding.timeout,
                        status_message: binding.status_message.clone(),
                        options: binding.options.clone(),
                    })
                    .collect(),
            })
            .command,
            hook.bindings.iter().map(|binding| HookBinding {
                harness: binding.harness.clone(),
                matcher: binding.matcher.clone(),
                command_prefix: binding.command_prefix.clone(),
                timeout: binding.timeout,
                status_message: binding.status_message.clone(),
                options: binding.options.clone(),
            }),
        );
    }
    if let Some(hook) = derived_path_zone_hook(manifest, paths) {
        builder.hook(hook.command, hook.bindings);
    }
    Some(builder.build())
}

fn derived_path_zone_hook(
    manifest: &AgentHooksConfigManifest,
    paths: Option<&PathZonesManifest>,
) -> Option<AgentHooksConfigHook> {
    let paths = paths?;
    if paths.generated.is_empty() && paths.read_only.is_empty() {
        return None;
    }

    let mut bindings = Vec::new();
    for harness in &manifest.harnesses {
        if let Some(binding) = derived_path_zone_binding(harness, manifest) {
            bindings.push(binding);
        }
    }
    if bindings.is_empty() {
        return None;
    }

    let guard = PathZoneGuard::new(paths.clone());
    Some(AgentHooksConfigHook {
        command: HookCommandSpec::new("guard-path-zones", HookStage::PreTool, move |event| {
            Ok(guard.check_event(event))
        }),
        bindings,
    })
}

fn derived_path_zone_binding(
    harness: &str,
    manifest: &AgentHooksConfigManifest,
) -> Option<HookBinding> {
    let command_prefix = manifest
        .targets
        .iter()
        .find(|target| target.harness == harness)
        .map(|target| target.command_prefix.clone());
    let (matcher, status_message) = match harness {
        "pi" => ("Edit|Write", "Checking path zones"),
        "codex" => (
            "^(apply_patch|Edit|Write|MultiEdit)$",
            "Checking path zones",
        ),
        "claude" => ("Edit|Write|MultiEdit", "Checking path zones..."),
        _ => return None,
    };

    Some(HookBinding {
        harness: harness.to_owned(),
        matcher: Some(matcher.to_owned()),
        command_prefix,
        timeout: 10,
        status_message: status_message.to_owned(),
        options: serde_json::Map::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathZoneGuard {
    generated: Vec<PathZoneRule>,
    read_only: Vec<PathZoneRule>,
}

impl PathZoneGuard {
    fn new(paths: PathZonesManifest) -> Self {
        Self {
            generated: paths.generated.into_iter().map(PathZoneRule::new).collect(),
            read_only: paths.read_only.into_iter().map(PathZoneRule::new).collect(),
        }
    }

    fn check_event(&self, event: &HookEvent) -> HookOutcome {
        for path in &event.touched_path_candidates {
            let normalized = normalize_candidate_path(path, &event.cwd);
            if let Some(zone) = self.generated.iter().find(|zone| zone.matches(&normalized)) {
                return HookOutcome::block(format!(
                    "Generated files must not be edited directly: {normalized}. {zone} is generated; edit the source and re-run sync."
                ));
            }
            if let Some(zone) = self.read_only.iter().find(|zone| zone.matches(&normalized)) {
                return HookOutcome::block(format!(
                    "Read-only path zone {zone} must not be edited: {normalized}."
                ));
            }
        }
        HookOutcome::pass()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathZoneRule {
    raw: String,
    normalized: String,
    kind: PathZoneRuleKind,
}

impl PathZoneRule {
    fn new(raw: String) -> Self {
        let normalized = normalize_manifest_path(&raw);
        let kind = if normalized.ends_with('/') {
            PathZoneRuleKind::Prefix
        } else {
            PathZoneRuleKind::Exact
        };
        Self {
            raw,
            normalized,
            kind,
        }
    }

    fn matches(&self, path: &str) -> bool {
        match self.kind {
            PathZoneRuleKind::Exact => path == self.normalized,
            PathZoneRuleKind::Prefix => path.starts_with(&self.normalized),
        }
    }
}

impl std::fmt::Display for PathZoneRule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathZoneRuleKind {
    Exact,
    Prefix,
}

fn normalize_manifest_path(path: &str) -> String {
    let mut normalized = normalize_slashes(path);
    while normalized.starts_with("./") {
        normalized = normalized[2..].to_owned();
    }
    normalized
}

fn normalize_candidate_path(path: &str, cwd: &str) -> String {
    let normalized = normalize_manifest_path(path);
    let cwd = normalize_manifest_path(cwd);
    let cwd_prefix = format!("{}/", cwd.trim_end_matches('/'));
    normalized
        .strip_prefix(&cwd_prefix)
        .unwrap_or(&normalized)
        .to_owned()
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn load_hook_command(
    command: &HookCommandManifest,
    config: &super::RunweaverConfig,
    registry: &BuiltinRegistry,
    missing: &mut MissingBuiltins,
) -> Option<HookCommandSpec> {
    if let Some(pipeline) = command.builtin.strip_prefix("agentsPostEdit:") {
        let pipeline = pipeline.to_owned();
        let config = config.clone();
        let mut spec =
            HookCommandSpec::new(command.name.clone(), HookStage::PostEdit, move |event| {
                crate::surfaces::agent_hooks::run_post_edit_pipeline(&config, &pipeline, event)
            });
        if !command.harnesses.is_empty() {
            spec = spec.with_harnesses(command.harnesses.clone());
        }
        return Some(spec);
    }
    if let Some(pipeline) = command.builtin.strip_prefix("agentsStop:") {
        let pipeline = pipeline.to_owned();
        let config = config.clone();
        let mut spec = HookCommandSpec::new(command.name.clone(), HookStage::Stop, move |event| {
            crate::surfaces::agent_hooks::run_stop_pipeline(&config, &pipeline, event)
        });
        if !command.harnesses.is_empty() {
            spec = spec.with_harnesses(command.harnesses.clone());
        }
        return Some(spec);
    }

    let run = registry
        .hook_commands
        .get(&command.builtin)
        .cloned()
        .or_else(|| missing.record("hookCommand", &command.builtin))?;
    let mut spec =
        HookCommandSpec::new(command.name.clone(), command.stage, move |event| run(event));
    if !command.harnesses.is_empty() {
        spec = spec.with_harnesses(command.harnesses.clone());
    }
    Some(spec)
}

fn generated_surface_files(
    manifest: &RunweaverDefinitionManifest,
    project_binary: &RunweaverProjectBinary,
) -> Vec<GeneratedSurfaceFile> {
    let mut files = Vec::new();
    let Some(surfaces) = &manifest.surfaces else {
        return files;
    };
    if let Some(git) = &surfaces.git {
        files.extend(render_git_hook_files(git, project_binary));
    }
    if let Some(ci) = &surfaces.ci
        && let Some(github) = &ci.github
        && let Some(pipeline) = &github.pull_request
    {
        files.push(GeneratedSurfaceFile {
            path: ".github/workflows/runweaver.yml".to_owned(),
            content: render_github_pull_request_workflow(pipeline, project_binary),
            executable: false,
        });
    }
    files
}

fn render_git_hook_files(
    git: &GitSurfaceManifest,
    project_binary: &RunweaverProjectBinary,
) -> Vec<GeneratedSurfaceFile> {
    let mut files = Vec::new();
    if git.pre_commit.is_some() {
        files.push(git_hook_file(
            "pre-commit",
            render_git_pre_commit_hook(project_binary),
        ));
    }
    if git.commit_msg.is_some() {
        files.push(git_hook_file(
            "commit-msg",
            render_git_commit_msg_hook(project_binary),
        ));
    }
    if git.pre_push.is_some() {
        files.push(git_hook_file(
            "pre-push",
            render_git_pre_push_hook(project_binary),
        ));
    }
    if git.post_commit.is_some() {
        files.push(git_hook_file(
            "post-commit",
            render_git_post_commit_hook(project_binary),
        ));
    }
    files
}

fn git_hook_file(name: &str, content: String) -> GeneratedSurfaceFile {
    GeneratedSurfaceFile {
        path: format!(".runweaver/git-hooks/{name}"),
        content,
        executable: true,
    }
}

fn render_git_pre_commit_hook(project_binary: &RunweaverProjectBinary) -> String {
    git_hook_script("pre-commit", "", project_binary)
}

fn render_git_commit_msg_hook(project_binary: &RunweaverProjectBinary) -> String {
    git_hook_script(
        "commit-msg",
        "\"${1:?commit message file is required}\"",
        project_binary,
    )
}

fn render_git_pre_push_hook(project_binary: &RunweaverProjectBinary) -> String {
    git_hook_script("pre-push", "", project_binary)
}

fn render_git_post_commit_hook(project_binary: &RunweaverProjectBinary) -> String {
    git_hook_script("post-commit", "", project_binary)
}

fn git_hook_script(
    slot: &str,
    extra_args: &str,
    project_binary: &RunweaverProjectBinary,
) -> String {
    let suffix = if extra_args.is_empty() {
        String::new()
    } else {
        format!(" {extra_args}")
    };
    let local_binary = format!("local_binary=\"$repo_root/{}\"", project_binary.out_path);
    let canonical_binary = format!(
        "  canonical_binary=\"$canonical_root/{}\"",
        project_binary.out_path
    );
    let fallback = format!(
        "  exec {} git-hook {{slot}}{{suffix}}",
        project_binary.fallback_command
    );
    [
        "#!/usr/bin/env sh",
        "set -eu",
        "",
        "# Pi worktree hook env isolation: let nested git commands rediscover cwd.",
        "git_local_env=\"$(git rev-parse --local-env-vars 2>/dev/null || true)\"",
        "if [ -n \"$git_local_env\" ]; then",
        "  unset $git_local_env",
        "fi",
        "",
        "repo_root=\"$(git rev-parse --show-toplevel)\"",
        "common_git_dir=\"$(git rev-parse --git-common-dir 2>/dev/null || true)\"",
        "absolute_common_git_dir=\"\"",
        "case \"$common_git_dir\" in",
        "  /*) absolute_common_git_dir=\"$common_git_dir\" ;;",
        "  .git) absolute_common_git_dir=\"$repo_root/.git\" ;;",
        "  \"\") absolute_common_git_dir=\"\" ;;",
        "  *) absolute_common_git_dir=\"$repo_root/$common_git_dir\" ;;",
        "esac",
        "canonical_root=\"\"",
        "case \"$absolute_common_git_dir\" in",
        "  */.git) canonical_root=\"$(dirname \"$absolute_common_git_dir\")\" ;;",
        "esac",
        "",
        "cd \"$repo_root\"",
        &local_binary,
        "canonical_binary=\"\"",
        "if [ -n \"$canonical_root\" ]; then",
        &canonical_binary,
        "fi",
        "",
        "if [ -x \"$local_binary\" ]; then",
        "  exec \"$local_binary\" git-hook {slot}{suffix}",
        "elif [ -n \"$canonical_binary\" ] && [ -x \"$canonical_binary\" ]; then",
        "  exec \"$canonical_binary\" git-hook {slot}{suffix}",
        "else",
        &fallback,
        "fi",
        "",
    ]
    .join("\n")
    .replace("{slot}", slot)
    .replace("{suffix}", &suffix)
}

fn render_github_pull_request_workflow(
    pipeline: &str,
    project_binary: &RunweaverProjectBinary,
) -> String {
    format!(
        "name: Runweaver\n\non:\n  pull_request:\n\njobs:\n  runweaver:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - uses: oven-sh/setup-bun@v2\n      - uses: dtolnay/rust-toolchain@stable\n      - run: bun install --frozen-lockfile\n      - run: {} run {pipeline}\n",
        project_binary.fallback_command
    )
}

#[derive(Debug, Default)]
struct MissingBuiltins {
    by_kind: BTreeMap<&'static str, BTreeSet<String>>,
}

impl MissingBuiltins {
    fn record<T>(&mut self, kind: &'static str, name: &str) -> Option<T> {
        self.by_kind
            .entry(kind)
            .or_default()
            .insert(name.to_owned());
        None
    }

    fn into_result(self) -> Result<(), ManifestLoadError> {
        if self.by_kind.is_empty() {
            return Ok(());
        }
        let lines = self
            .by_kind
            .into_iter()
            .flat_map(|(kind, names)| {
                names
                    .into_iter()
                    .map(move |name| format!("  - {kind}: {name}"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        Err(ManifestLoadError::UnknownBuiltins(lines))
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ExecutionContext;
    use crate::surfaces::agent_hooks::{HookEvent, HookOutcome, codex_harness};

    use super::*;

    fn test_project_binary() -> RunweaverProjectBinary {
        RunweaverProjectBinary {
            package: "demo-rs".to_owned(),
            binary_name: "demo-rs".to_owned(),
            out_path: ".runweaver/bin/demo".to_owned(),
            hooks_config_name: "demo-hooks".to_owned(),
            fallback_command: "cargo run --quiet -p demo-rs --".to_owned(),
        }
    }

    fn hook_event(paths: &[&str]) -> HookEvent {
        HookEvent {
            harness: "codex".to_owned(),
            stage: HookStage::PreTool,
            session_id: "session".to_owned(),
            tool_call_id: Some("tool".to_owned()),
            transcript_path: None,
            cwd: "/repo".to_owned(),
            touched_path_candidates: paths.iter().map(|path| (*path).to_owned()).collect(),
            patch_text: None,
            tool_command: None,
            tool_name: Some("Edit".to_owned()),
            tool_response: None,
            stop_hook_active: false,
        }
    }

    fn selectors(
        extensions: &[&str],
        files: &[&str],
        prefixes: &[&str],
        fallback: &[&str],
    ) -> ToolTargetsManifest {
        ToolTargetsManifest::Selectors(FileTargetsManifest {
            extensions: strings(extensions),
            files: strings(files),
            prefixes: strings(prefixes),
            fallback: strings(fallback),
        })
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn assert_preset_targets(preset: &str, targets: ToolTargetsManifest, expected: ToolTargets) {
        let tool = preset_tool(preset, &[], Some(&targets), &[]);
        assert_eq!(tool.targets, expected);
    }

    #[test]
    fn load_runweaver_definition_resolves_tool_pipeline_contract() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: Some(PathZonesManifest {
                writable: vec!["src/".to_owned()],
                check_only: vec![".codex/".to_owned()],
                ..PathZonesManifest::default()
            }),
            tools: BTreeMap::from([
                (
                    "fmt".to_owned(),
                    ToolDefinitionManifest::Declarative {
                        targets: Some(ToolTargetsManifest::Selectors(FileTargetsManifest {
                            extensions: vec!["rs".to_owned()],
                            files: Vec::new(),
                            prefixes: vec!["src/".to_owned(), ".codex/".to_owned()],
                            fallback: vec!["src/".to_owned(), ".codex/".to_owned()],
                        })),
                        check: vec!["/bin/true".to_owned(), "{files}".to_owned()],
                        fix: Some(vec!["/bin/true".to_owned(), "{files}".to_owned()]),
                        diagnostics: DiagnosticsParserManifest::Named {
                            parser: NamedDiagnosticsParserManifest::Unix,
                        },
                        affected: vec!["tests/{stem}_test.rs".to_owned()],
                    },
                ),
                (
                    "script".to_owned(),
                    ToolDefinitionManifest::Script {
                        script: "true".to_owned(),
                    },
                ),
            ]),
            pipelines: BTreeMap::from([
                (
                    "check".to_owned(),
                    PipelineDefinitionManifest::Check {
                        check: vec!["fmt".to_owned(), "script".to_owned()],
                    },
                ),
                (
                    "autofix".to_owned(),
                    PipelineDefinitionManifest::Fix {
                        fix: vec!["fmt".to_owned()],
                        then: Some(Box::new(PipelineDefinitionManifest::Check {
                            check: vec!["fmt".to_owned()],
                        })),
                    },
                ),
                (
                    "validate".to_owned(),
                    PipelineDefinitionManifest::Stages {
                        stages: vec!["check".to_owned()],
                    },
                ),
            ]),
            operations: BTreeMap::new(),
            surfaces: None,
            bindings: Vec::new(),
        };

        let definition =
            load_runweaver_definition(&manifest, &BuiltinRegistry::new(), &test_project_binary())
                .expect("v2 should load");
        let run = crate::runtime::run_task(
            &definition.task_config(),
            "check",
            ExecutionContext::new(".").with_files(vec!["src/lib.rs".to_owned()]),
        )
        .expect("pipeline should run");

        assert_eq!(run.status, crate::config::TaskRunStatus::Completed);
        assert_eq!(run.completion, Some(crate::config::TaskCompletion::Success));
        assert!(
            definition
                .task_config()
                .tasks
                .contains_key("__tool:check:fmt:check")
        );
        assert!(
            definition
                .task_config()
                .tasks
                .contains_key("__pipeline:autofix:then")
        );
    }

    #[test]
    fn parser_specs_attach_structured_diagnostics_to_tool_failures() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: None,
            tools: BTreeMap::from([(
                "deptry".to_owned(),
                ToolDefinitionManifest::Declarative {
                    targets: Some(ToolTargetsManifest::Selectors(FileTargetsManifest {
                        extensions: vec!["py".to_owned()],
                        files: Vec::new(),
                        prefixes: vec!["src/".to_owned()],
                        fallback: vec!["src/".to_owned()],
                    })),
                    check: vec![
                        "/bin/sh".to_owned(),
                        "-c".to_owned(),
                        "echo 'src/app.py:1:2: DEP001 bad import'; exit 1".to_owned(),
                    ],
                    fix: None,
                    diagnostics: DiagnosticsParserManifest::Regex {
                        parser: NamedDiagnosticsParserManifest::Regex,
                        pattern: "^(?<file>.+?):(?<line>\\d+):(?<col>\\d+): (?<code>DEP\\d+) (?<message>.+)$".to_owned(),
                    },
                    affected: Vec::new(),
                },
            )]),
            pipelines: BTreeMap::from([(
                "hygiene".to_owned(),
                PipelineDefinitionManifest::Check {
                    check: vec!["deptry".to_owned()],
                },
            )]),
            operations: BTreeMap::new(),
            surfaces: None,
            bindings: Vec::new(),
        };

        let definition =
            load_runweaver_definition(&manifest, &BuiltinRegistry::new(), &test_project_binary())
                .expect("v2 should load");
        let run = crate::runtime::run_task(
            &definition.task_config(),
            "hygiene",
            ExecutionContext::new(".").with_files(vec!["src/app.py".to_owned()]),
        )
        .expect("pipeline should run");

        let child = &run.children[0];
        let diagnostics = child
            .data
            .as_ref()
            .and_then(|data| data.get("diagnostics"))
            .and_then(serde_json::Value::as_array)
            .expect("diagnostics should be attached");
        assert_eq!(diagnostics[0]["file"], "src/app.py");
        assert_eq!(diagnostics[0]["code"], "DEP001");
    }

    #[test]
    fn selectors_without_explicit_fallback_resolve_empty_fallback() {
        let targets =
            manifest_targets(Some(&ToolTargetsManifest::Selectors(FileTargetsManifest {
                extensions: vec!["rs".to_owned()],
                files: vec!["Cargo.toml".to_owned()],
                prefixes: vec!["crates/".to_owned()],
                fallback: Vec::new(),
            })));

        assert_eq!(
            targets,
            ToolTargets {
                extensions: vec!["rs".to_owned()],
                files: vec!["Cargo.toml".to_owned()],
                prefixes: vec!["crates/".to_owned()],
                fallback: Vec::new(),
            }
        );
    }

    #[test]
    fn oxfmt_preset_resolves_repo_targets_from_manifest() {
        assert_preset_targets(
            "oxfmt",
            selectors(
                &[],
                &[],
                &[],
                &[
                    ".runweaver/project-specific/**/*.{ts,tsx,js,jsx,mjs}",
                    "harness/**/*.{ts,tsx,js,jsx,mjs}",
                    "platform/**/*.{ts,tsx,js,jsx,mjs}",
                    ".runweaver/configs/commitlint.config.js",
                ],
            ),
            ToolTargets {
                extensions: strings(&["ts", "tsx", "js", "jsx", "mjs", "json", "jsonc"]),
                files: Vec::new(),
                prefixes: Vec::new(),
                fallback: strings(&[
                    ".runweaver/project-specific/**/*.{ts,tsx,js,jsx,mjs}",
                    "harness/**/*.{ts,tsx,js,jsx,mjs}",
                    "platform/**/*.{ts,tsx,js,jsx,mjs}",
                    ".runweaver/configs/commitlint.config.js",
                ]),
            },
        );
    }

    #[test]
    fn oxlint_preset_resolves_repo_targets_from_manifest() {
        assert_preset_targets(
            "oxlint",
            selectors(
                &[],
                &[".runweaver/configs/commitlint.config.js"],
                &[
                    ".runweaver/project-specific/",
                    "src/",
                    "harness/",
                    "platform/",
                    ".claude/",
                    ".pi/",
                    ".agents/",
                    ".codex/",
                ],
                &[".runweaver/project-specific/", "harness/", "platform/"],
            ),
            ToolTargets {
                extensions: strings(&["ts", "tsx", "js", "jsx", "mjs"]),
                files: strings(&[".runweaver/configs/commitlint.config.js"]),
                prefixes: strings(&[
                    ".runweaver/project-specific/",
                    "src/",
                    "harness/",
                    "platform/",
                    ".claude/",
                    ".pi/",
                    ".agents/",
                    ".codex/",
                ]),
                fallback: strings(&[".runweaver/project-specific/", "harness/", "platform/"]),
            },
        );
    }

    #[test]
    fn cargo_fmt_preset_resolves_repo_targets_from_manifest() {
        assert_preset_targets(
            "cargo-fmt",
            selectors(&[], &[], &["crates/"], &[]),
            ToolTargets {
                extensions: vec!["rs".to_owned()],
                files: Vec::new(),
                prefixes: vec!["crates/".to_owned()],
                fallback: Vec::new(),
            },
        );
    }

    #[test]
    fn cargo_clippy_preset_resolves_repo_targets_from_manifest() {
        assert_preset_targets(
            "cargo-clippy",
            selectors(&[], &["Cargo.toml", "Cargo.lock"], &["crates/"], &[]),
            ToolTargets {
                extensions: strings(&["rs", "toml", "lock"]),
                files: strings(&["Cargo.toml", "Cargo.lock"]),
                prefixes: vec!["crates/".to_owned()],
                fallback: Vec::new(),
            },
        );
    }

    #[test]
    fn presets_build_check_and_fix_invocations() {
        let zones = PathZonesManifest {
            writable: vec!["crates/".to_owned(), "src/".to_owned()],
            ..PathZonesManifest::default()
        };
        let cargo_fmt = preset_tool("cargo-fmt", &[], None, &[]);
        let rust_ctx = ExecutionContext::new(".").with_files(vec![
            "crates/core-rs/src/lib.rs".to_owned(),
            "crates/demo-rs/src/main.rs".to_owned(),
        ]);
        assert_eq!(
            manifest_tool_args(&cargo_fmt, ToolRunMode::Fix, &zones, &rust_ctx),
            Some(vec![
                "fmt".to_owned(),
                "-p".to_owned(),
                "core-rs".to_owned(),
                "-p".to_owned(),
                "demo-rs".to_owned(),
            ])
        );

        let non_rust_ctx = ExecutionContext::new(".").with_files(vec!["src/index.ts".to_owned()]);
        assert_eq!(
            manifest_tool_args(&cargo_fmt, ToolRunMode::Fix, &zones, &non_rust_ctx),
            None
        );

        let bun_test = preset_tool("bun-test", &[], None, &[]);
        let test_ctx =
            ExecutionContext::new(".").with_files(vec!["src/policies/scope.test.ts".to_owned()]);
        assert_eq!(
            manifest_tool_args(&bun_test, ToolRunMode::Check, &zones, &test_ctx),
            Some(vec![
                "test".to_owned(),
                "./src/policies/scope.test.ts".to_owned(),
            ])
        );

        let oxlint = preset_tool(
            "oxlint",
            &[
                "-c".to_owned(),
                ".runweaver/configs/oxlintrc.jsonc".to_owned(),
            ],
            None,
            &[],
        );
        let ts_ctx = ExecutionContext::new(".").with_files(vec!["src/index.ts".to_owned()]);
        assert_eq!(
            manifest_tool_args(&oxlint, ToolRunMode::Check, &zones, &ts_ctx),
            Some(vec![
                "-c".to_owned(),
                ".runweaver/configs/oxlintrc.jsonc".to_owned(),
                "--quiet".to_owned(),
                "--format=unix".to_owned(),
                "src/index.ts".to_owned(),
            ])
        );

        let cargo_check = preset_tool(
            "cargo-check",
            &[
                "--workspace".to_owned(),
                "--all-targets".to_owned(),
                "--all-features".to_owned(),
                "--locked".to_owned(),
            ],
            None,
            &[],
        );
        assert_eq!(
            manifest_tool_args(&cargo_check, ToolRunMode::Check, &zones, &rust_ctx),
            Some(vec![
                "check".to_owned(),
                "--workspace".to_owned(),
                "--all-targets".to_owned(),
                "--all-features".to_owned(),
                "--locked".to_owned(),
            ])
        );

        let mut cargo_hook_ctx = rust_ctx.clone();
        cargo_hook_ctx.input = Some(serde_json::json!({
            "args": ["Cargo.toml", "crates/core-rs/src/lib.rs"]
        }));
        assert_eq!(
            manifest_tool_args(&cargo_check, ToolRunMode::Check, &zones, &cargo_hook_ctx),
            Some(vec![
                "check".to_owned(),
                "--workspace".to_owned(),
                "--all-targets".to_owned(),
                "--all-features".to_owned(),
                "--locked".to_owned(),
            ])
        );
    }

    #[test]
    fn python_project_tool_pipeline_shape_is_schema_expressible() {
        let value = serde_json::json!({
            "version": 2,
            "paths": {
                "writable": ["src/", "tests/", "scripts/", "pyproject.toml"],
                "generated": ["uv.lock"],
                "readOnly": ["migrations/versions/"]
            },
            "tools": {
                "ruff.lint": { "preset": "ruff" },
                "ruff.fmt": { "preset": "ruff-format" },
                "mypy": { "preset": "mypy", "args": ["--strict"] },
                "pytest": {
                    "preset": "pytest",
                    "targets": ["tests/"],
                    "affected": ["tests/test_{stem}.py", "tests/{dir}/test_{stem}.py"]
                },
                "deptry": {
                    "targets": { "extensions": ["py"] },
                    "check": ["deptry", "src"],
                    "diagnostics": {
                        "parser": "regex",
                        "pattern": "^(?<file>.+?):(?<line>\\d+):(?<col>\\d+): (?<code>DEP\\d+) (?<message>.+)$"
                    }
                },
                "committed": {
                    "check": ["committed", "--commit-msg-file"],
                    "diagnostics": { "parser": "unix" }
                },
                "lockCheck": { "script": "uv lock --check" }
            },
            "pipelines": {
                "check": { "check": ["ruff.fmt", "ruff.lint", "mypy", "pytest"] },
                "hygiene": { "check": ["deptry", "lockCheck"] },
                "validate": { "stages": ["check", "hygiene"] },
                "autofix": {
                    "fix": ["ruff.lint", "ruff.fmt"],
                    "then": { "check": ["ruff.lint"] }
                }
            },
            "operations": {},
            "bindings": []
        });
        let manifest: RunweaverDefinitionManifest =
            serde_json::from_value(value).expect("python shape should deserialize");
        assert!(manifest.tools.contains_key("deptry"));
        assert!(manifest.pipelines.contains_key("autofix"));
    }

    #[test]
    fn path_zone_guard_matches_exact_files_prefixes_and_absolute_cwd_paths() {
        let guard = PathZoneGuard::new(PathZonesManifest {
            generated: vec!["settings.managed.json".to_owned()],
            read_only: vec!["vendor/reference/".to_owned()],
            ..PathZonesManifest::default()
        });

        assert!(matches!(
            guard.check_event(&hook_event(&["src/index.ts"])),
            HookOutcome::Pass { .. }
        ));
        assert!(matches!(
            guard.check_event(&hook_event(&["/repo/settings.managed.json"])),
            HookOutcome::Block { ref reason, .. } if reason.contains("settings.managed.json is generated")
        ));
        assert!(matches!(
            guard.check_event(&hook_event(&[
                "vendor/reference/example/src/index.ts"
            ])),
            HookOutcome::Block { ref reason, .. } if reason.contains("Read-only path zone vendor/reference/")
        ));
    }

    #[test]
    fn default_builtin_registry_resolves_built_in_harnesses_and_destructive_guard() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: None,
            tools: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            operations: BTreeMap::new(),
            surfaces: Some(SurfacesManifest {
                agents: Some(AgentsSurfaceManifest {
                    harnesses: vec!["claude".to_owned(), "codex".to_owned()],
                    pre_tool: vec![AgentsPreToolGuardManifest::Builtin {
                        guard: AgentsBuiltinGuardManifest::DestructiveCommands,
                    }],
                    post_edit: None,
                    stop: None,
                }),
                git: None,
                ci: None,
                cli: None,
            }),
            bindings: Vec::new(),
        };

        let loaded = load_runweaver_manifest(
            &manifest,
            &default_builtin_registry(),
            &test_project_binary(),
        )
        .expect("default registry should resolve built-in harnesses and guard");
        let hooks = loaded.agent_hooks.expect("hooks should be present");

        let mut destructive_event = hook_event(&[]);
        destructive_event.tool_name = Some("Bash".to_owned());
        destructive_event.tool_command = Some("git reset --hard".to_owned());
        for harness in ["claude", "codex"] {
            let command = hooks
                .app
                .command("guard-destructive", harness)
                .expect("guard-destructive should be bound");
            assert!(matches!(
                command.run(&destructive_event).unwrap(),
                HookOutcome::Block { ref reason, .. } if reason.contains("git reset --hard")
            ));
        }
    }

    #[test]
    fn default_builtin_registry_reports_unknown_builtins_for_custom_harnesses() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: None,
            tools: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            operations: BTreeMap::new(),
            surfaces: Some(SurfacesManifest {
                agents: Some(AgentsSurfaceManifest {
                    harnesses: vec!["claude".to_owned(), "fixture".to_owned()],
                    pre_tool: Vec::new(),
                    post_edit: None,
                    stop: None,
                }),
                git: None,
                ci: None,
                cli: None,
            }),
            bindings: Vec::new(),
        };

        let error = load_runweaver_manifest(
            &manifest,
            &default_builtin_registry(),
            &test_project_binary(),
        )
        .expect_err("custom harness should be reported as missing builtin");

        assert_eq!(
            error,
            ManifestLoadError::UnknownBuiltins("  - harness: fixture".to_owned())
        );
    }

    #[test]
    fn load_manifest_derives_path_zone_guard_bindings_for_supported_harnesses() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: Some(PathZonesManifest {
                generated: vec!["settings.managed.json".to_owned()],
                read_only: vec!["vendor/reference/".to_owned()],
                writable: vec!["src/".to_owned()],
                check_only: vec![".codex/".to_owned()],
            }),
            tools: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            operations: BTreeMap::new(),
            surfaces: Some(SurfacesManifest {
                agents: Some(AgentsSurfaceManifest {
                    harnesses: vec!["codex".to_owned()],
                    pre_tool: Vec::new(),
                    post_edit: None,
                    stop: None,
                }),
                git: None,
                ci: None,
                cli: None,
            }),
            bindings: Vec::new(),
        };
        let registry = BuiltinRegistry::new().harness("codex", codex_harness());

        let loaded = load_runweaver_manifest(&manifest, &registry, &test_project_binary())
            .expect("manifest should load hooks");
        let hooks = loaded.agent_hooks.expect("hooks should be present");

        assert_eq!(hooks.name, "demo-hooks");
        assert_eq!(hooks.binary_name, "demo-rs hook");
        assert_eq!(hooks.hooks.len(), 1);
        assert_eq!(hooks.hooks[0].command.name(), "guard-path-zones");
        assert_eq!(
            hooks.hooks[0].bindings,
            vec![
                HookBinding::new("codex", 10, "Checking path zones")
                    .with_matcher("^(apply_patch|Edit|Write|MultiEdit)$")
                    .with_command_prefix(
                        "cd \"$(git rev-parse --show-toplevel)\" && ./.runweaver/bin/demo hook codex",
                    )
            ]
        );
        assert!(matches!(
            hooks
                .app
                .command("guard-path-zones", "codex")
                .unwrap()
                .run(&hook_event(&["settings.managed.json"]))
                .unwrap(),
            HookOutcome::Block { ref reason, .. }
                if reason.contains("Generated files must not be edited directly")
        ));
        assert!(matches!(
            hooks
                .app
                .command("guard-path-zones", "codex")
                .unwrap()
                .run(&hook_event(&["src/index.ts"]))
                .unwrap(),
            HookOutcome::Pass { .. }
        ));
    }

    #[test]
    fn manifest_schema_round_trips_paths_zones() {
        let value = serde_json::json!({
            "version": 2,
            "paths": {
                "writable": ["src/"],
                "checkOnly": [".codex/"],
                "generated": ["settings.managed.json"],
                "readOnly": ["vendor/reference/"]
            },
            "tools": {},
            "pipelines": {},
            "operations": {},
            "bindings": []
        });
        let manifest: RunweaverDefinitionManifest =
            serde_json::from_value(value).expect("paths manifest should deserialize");

        assert_eq!(
            manifest.paths.expect("paths should be present"),
            PathZonesManifest {
                writable: vec!["src/".to_owned()],
                check_only: vec![".codex/".to_owned()],
                generated: vec!["settings.managed.json".to_owned()],
                read_only: vec!["vendor/reference/".to_owned()],
            }
        );
        assert!(
            runweaver_manifest_json_schema()
                .to_string()
                .contains("Path zones used by Runweaver surfaces")
        );
    }

    #[test]
    fn manifest_schema_round_trips_git_ci_cli_surfaces() {
        let value = serde_json::json!({
            "version": 2,
            "tools": {
                "commitlint": { "preset": "commitlint", "args": ["--config", ".runweaver/configs/commitlint.config.js", "--edit"] }
            },
            "pipelines": {
                "autofix": { "fix": ["commitlint"] },
                "validate": { "check": ["commitlint"] }
            },
            "operations": {},
            "surfaces": {
                "git": {
                    "preCommit": { "run": "autofix", "files": "staged", "also": ["commitlint"] },
                    "commitMsg": { "tool": "commitlint" },
                    "prePush": { "run": "validate" },
                    "postCommit": { "tool": "commitlint" }
                },
                "ci": { "github": { "pullRequest": "validate" } },
                "cli": true
            },
            "bindings": []
        });

        let manifest: RunweaverDefinitionManifest =
            serde_json::from_value(value).expect("git/ci/cli surfaces should deserialize");
        let surfaces = manifest.surfaces.expect("surfaces should be present");
        let git = surfaces.git.expect("git surface should be present");

        assert_eq!(
            git.pre_commit.expect("preCommit").files,
            Some(GitFilesScopeManifest::Staged)
        );
        assert_eq!(git.commit_msg.expect("commitMsg").tool, "commitlint");
        assert_eq!(
            surfaces
                .ci
                .expect("ci")
                .github
                .expect("github")
                .pull_request
                .as_deref(),
            Some("validate")
        );
        assert_eq!(surfaces.cli, Some(true));
    }

    #[test]
    fn generated_git_hook_scripts_invoke_project_binary_and_commit_msg_trailing_arg() {
        let manifest = RunweaverDefinitionManifest {
            version: RUNWEAVER_DEFINITION_MANIFEST_VERSION,
            paths: None,
            tools: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            operations: BTreeMap::new(),
            surfaces: Some(SurfacesManifest {
                agents: None,
                git: Some(GitSurfaceManifest {
                    pre_commit: Some(GitPreCommitSlotManifest {
                        run: "autofix".to_owned(),
                        files: Some(GitFilesScopeManifest::Staged),
                        also: vec!["gitleaks".to_owned()],
                    }),
                    commit_msg: Some(GitToolSlotManifest {
                        tool: "commitlint".to_owned(),
                    }),
                    pre_push: Some(GitPipelineSlotManifest {
                        run: "validate".to_owned(),
                    }),
                    post_commit: Some(GitToolSlotManifest {
                        tool: "installPiConfig".to_owned(),
                    }),
                }),
                ci: Some(CiSurfaceManifest {
                    github: Some(GithubCiSurfaceManifest {
                        pull_request: Some("validate".to_owned()),
                    }),
                }),
                cli: Some(true),
            }),
            bindings: Vec::new(),
        };

        let files = generated_surface_files(&manifest, &test_project_binary());
        let commit_msg = files
            .iter()
            .find(|file| file.path == ".runweaver/git-hooks/commit-msg")
            .expect("commit-msg hook should be generated");

        assert!(commit_msg.executable);
        assert!(commit_msg.content.contains("unset $git_local_env"));
        assert!(
            commit_msg
                .content
                .contains("local_binary=\"$repo_root/.runweaver/bin/demo\"")
        );
        assert!(
            commit_msg
                .content
                .contains("canonical_binary=\"$canonical_root/.runweaver/bin/demo\"")
        );
        assert!(
            commit_msg
                .content
                .contains("exec \"$canonical_binary\" git-hook commit-msg \"${1:?commit message file is required}\"")
        );
        assert!(
            commit_msg
                .content
                .contains("exec cargo run --quiet -p demo-rs -- git-hook commit-msg \"${1:?commit message file is required}\"")
        );
        assert!(files.iter().any(|file| {
            file.path == ".github/workflows/runweaver.yml"
                && file
                    .content
                    .contains("cargo run --quiet -p demo-rs -- run validate")
        }));
    }

    #[test]
    fn commitlint_preset_appends_commit_message_path_as_trailing_arg() {
        let tool = preset_tool(
            "commitlint",
            &[
                "--config".to_owned(),
                ".runweaver/configs/commitlint.config.js".to_owned(),
                "--edit".to_owned(),
            ],
            None,
            &[],
        );
        let mut ctx = ExecutionContext::new(".");
        ctx.input = Some(serde_json::json!({ "args": [".git/COMMIT_EDITMSG"] }));

        assert_eq!(
            manifest_tool_args(
                &tool,
                ToolRunMode::Check,
                &PathZonesManifest::default(),
                &ctx
            ),
            Some(vec![
                "--config".to_owned(),
                ".runweaver/configs/commitlint.config.js".to_owned(),
                "--edit".to_owned(),
                ".git/COMMIT_EDITMSG".to_owned(),
            ])
        );
    }
}
