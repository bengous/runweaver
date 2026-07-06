use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use super::contract::HookStage;

pub type HarnessOptions = Map<String, Value>;
pub type HarnessHookConfigRegistry = BTreeMap<String, HarnessHookConfig>;

#[derive(Debug, Clone, PartialEq)]
pub struct HarnessTarget {
    pub harness: String,
    pub path: String,
    pub command_prefix: String,
    pub options: HarnessOptions,
}

impl HarnessTarget {
    pub fn new(
        harness: impl Into<String>,
        path: impl Into<String>,
        command_prefix: impl Into<String>,
    ) -> Self {
        Self {
            harness: harness.into(),
            path: path.into(),
            command_prefix: command_prefix.into(),
            options: HarnessOptions::new(),
        }
    }

    pub fn with_option(mut self, key: impl Into<String>, value: Value) -> Self {
        self.options.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookBinding {
    pub harness: String,
    pub matcher: Option<String>,
    pub command_prefix: Option<String>,
    pub timeout: u32,
    pub status_message: String,
    pub options: HarnessOptions,
}

impl HookBinding {
    pub fn new(
        harness: impl Into<String>,
        timeout: u32,
        status_message: impl Into<String>,
    ) -> Self {
        Self {
            harness: harness.into(),
            matcher: None,
            command_prefix: None,
            timeout,
            status_message: status_message.into(),
            options: HarnessOptions::new(),
        }
    }

    pub fn with_matcher(mut self, matcher: impl Into<String>) -> Self {
        self.matcher = Some(matcher.into());
        self
    }

    pub fn with_command_prefix(mut self, command_prefix: impl Into<String>) -> Self {
        self.command_prefix = Some(command_prefix.into());
        self
    }

    pub fn with_option(mut self, key: impl Into<String>, value: Value) -> Self {
        self.options.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookConfigCommand {
    pub name: String,
    pub stage: HookStage,
    pub bindings: Vec<HookBinding>,
}

impl HookConfigCommand {
    pub fn new(name: impl Into<String>, stage: HookStage, bindings: Vec<HookBinding>) -> Self {
        Self {
            name: name.into(),
            stage,
            bindings,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HarnessHookConfigSet {
    pub source_path: String,
    pub hook_configs: HarnessHookConfigRegistry,
    pub targets: Vec<HarnessTarget>,
    pub hooks: Vec<HookConfigCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessHookFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessHookConfigMismatch {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessHookConfigCheckResult {
    pub ok: bool,
    pub mismatches: Vec<HarnessHookConfigMismatch>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HarnessHookCommand {
    pub command_type: String,
    pub command: String,
    pub timeout: u32,
    pub status_message: String,
    pub binding_options: HarnessOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HarnessHookGroup {
    pub stage: String,
    pub matcher: Option<String>,
    pub hooks: Vec<HarnessHookCommand>,
}

pub struct HarnessHookConfigRenderInput<'a> {
    pub source_path: &'a str,
    pub target: &'a HarnessTarget,
    pub groups: &'a [HarnessHookGroup],
}

pub struct HarnessHookConfigWriteInput<'a> {
    pub path: &'a Path,
    pub source_path: &'a str,
    pub target: &'a HarnessTarget,
    pub groups: &'a [HarnessHookGroup],
    pub owned_command_prefixes: &'a [String],
    pub existing_content: Option<&'a str>,
}

pub struct HarnessHookConfigCheckInput<'a> {
    pub path: &'a Path,
    pub source_path: &'a str,
    pub target: &'a HarnessTarget,
    pub groups: &'a [HarnessHookGroup],
    pub owned_command_prefixes: &'a [String],
    pub actual_content: &'a str,
}

pub struct HarnessHookConfigCheckProjection {
    pub expected: String,
    pub actual: String,
}

pub struct HookBindingValidationInput<'a> {
    pub hook: &'a HookConfigCommand,
    pub binding: &'a HookBinding,
}

pub type HarnessHookConfigRenderFn = Arc<
    dyn for<'a> Fn(HarnessHookConfigRenderInput<'a>) -> Result<String, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
>;
pub type HarnessHookConfigWriteFn = Arc<
    dyn for<'a> Fn(HarnessHookConfigWriteInput<'a>) -> Result<String, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
>;
pub type HarnessHookConfigCheckFn = Arc<
    dyn for<'a> Fn(
            HarnessHookConfigCheckInput<'a>,
        ) -> Result<HarnessHookConfigCheckProjection, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
>;
pub type HarnessTargetValidationFn =
    Arc<dyn Fn(&HarnessTarget) -> Result<(), HarnessHookConfigError> + Send + Sync + 'static>;
pub type HookBindingValidationFn = Arc<
    dyn for<'a> Fn(HookBindingValidationInput<'a>) -> Result<(), HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
>;

#[derive(Clone)]
pub struct HarnessHookConfig {
    pub default_path: String,
    render: HarnessHookConfigRenderFn,
    write: Option<HarnessHookConfigWriteFn>,
    check: Option<HarnessHookConfigCheckFn>,
    validate_target: Option<HarnessTargetValidationFn>,
    validate_binding: Option<HookBindingValidationFn>,
}

impl HarnessHookConfig {
    pub fn new(
        default_path: impl Into<String>,
        render: impl for<'a> Fn(
            HarnessHookConfigRenderInput<'a>,
        ) -> Result<String, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            default_path: default_path.into(),
            render: Arc::new(render),
            write: None,
            check: None,
            validate_target: None,
            validate_binding: None,
        }
    }

    pub fn with_write(
        mut self,
        write: impl for<'a> Fn(
            HarnessHookConfigWriteInput<'a>,
        ) -> Result<String, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.write = Some(Arc::new(write));
        self
    }

    pub fn with_check(
        mut self,
        check: impl for<'a> Fn(
            HarnessHookConfigCheckInput<'a>,
        )
            -> Result<HarnessHookConfigCheckProjection, HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.check = Some(Arc::new(check));
        self
    }

    pub fn with_validate_target(
        mut self,
        validate: impl Fn(&HarnessTarget) -> Result<(), HarnessHookConfigError> + Send + Sync + 'static,
    ) -> Self {
        self.validate_target = Some(Arc::new(validate));
        self
    }

    pub fn with_validate_binding(
        mut self,
        validate: impl for<'a> Fn(HookBindingValidationInput<'a>) -> Result<(), HarnessHookConfigError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.validate_binding = Some(Arc::new(validate));
        self
    }

    pub fn render(
        &self,
        input: HarnessHookConfigRenderInput<'_>,
    ) -> Result<String, HarnessHookConfigError> {
        (self.render)(input)
    }

    pub fn write_content(
        &self,
        input: HarnessHookConfigWriteInput<'_>,
    ) -> Result<String, HarnessHookConfigError> {
        match &self.write {
            Some(write) => write(input),
            None => self.render(HarnessHookConfigRenderInput {
                source_path: input.source_path,
                target: input.target,
                groups: input.groups,
            }),
        }
    }

    pub fn check_projection(
        &self,
        input: HarnessHookConfigCheckInput<'_>,
    ) -> Result<HarnessHookConfigCheckProjection, HarnessHookConfigError> {
        match &self.check {
            Some(check) => check(input),
            None => Ok(HarnessHookConfigCheckProjection {
                expected: self.render(HarnessHookConfigRenderInput {
                    source_path: input.source_path,
                    target: input.target,
                    groups: input.groups,
                })?,
                actual: input.actual_content.to_owned(),
            }),
        }
    }

    pub fn validate_target(&self, target: &HarnessTarget) -> Result<(), HarnessHookConfigError> {
        match &self.validate_target {
            Some(validate) => validate(target),
            None => Ok(()),
        }
    }

    pub fn validate_binding(
        &self,
        input: HookBindingValidationInput<'_>,
    ) -> Result<(), HarnessHookConfigError> {
        match &self.validate_binding {
            Some(validate) => validate(input),
            None => Ok(()),
        }
    }
}

impl std::fmt::Debug for HarnessHookConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HarnessHookConfig")
            .field("default_path", &self.default_path)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HarnessHookConfigError {
    #[error("Harness target {harness} does not define hook-config support.")]
    MissingHarnessConfig { harness: String },
    #[error("Harness target id must not be empty.")]
    EmptyTargetId,
    #[error("Harness target path must not be empty for {harness}.")]
    EmptyTargetPath { harness: String },
    #[error("Harness target command prefix must not be empty for {harness}.")]
    EmptyTargetCommandPrefix { harness: String },
    #[error("Duplicate harness target: {harness}")]
    DuplicateHarnessTarget { harness: String },
    #[error("Hook command name must not be empty.")]
    EmptyHookCommandName,
    #[error("Duplicate hook command: {command}")]
    DuplicateHookCommand { command: String },
    #[error("Hook command {command} references unknown harness: {harness}")]
    UnknownBindingHarness { command: String, harness: String },
    #[error("Hook command {command} must define a positive timeout.")]
    NonPositiveTimeout { command: String },
    #[error("{message}")]
    Custom { message: String },
    #[error("I/O error for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid JSON for {path}: {source}")]
    InvalidJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Invalid TOML for {path}: {source}")]
    InvalidToml {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
    #[error("Invalid hook config shape for {path}: {message}")]
    InvalidShape { path: PathBuf, message: String },
}

pub fn define_harness_hook_config(config: HarnessHookConfig) -> HarnessHookConfig {
    config
}

pub fn render_harness_hook_config_files(
    config: &HarnessHookConfigSet,
) -> Result<Vec<HarnessHookFile>, HarnessHookConfigError> {
    validate_harness_hook_config_set(config)?;
    config
        .targets
        .iter()
        .map(|target| {
            Ok(HarnessHookFile {
                path: target.path.clone(),
                content: render_harness_hook_config_file(config, target)?,
            })
        })
        .collect()
}

pub fn check_harness_hook_config_files(
    root: &Path,
    config: &HarnessHookConfigSet,
) -> Result<HarnessHookConfigCheckResult, HarnessHookConfigError> {
    validate_harness_hook_config_set(config)?;
    let mut mismatches = Vec::new();
    for target in &config.targets {
        let hook_config = hook_config_for_target(config, target)?;
        let groups = harness_hook_groups(config, target);
        let owned_command_prefixes = owned_command_prefixes(config, target);
        let file_path = root.join(&target.path);
        let actual = read_optional_to_string(&file_path)?;
        let Some(actual_content) = actual.as_deref() else {
            mismatches.push(HarnessHookConfigMismatch {
                path: target.path.clone(),
                expected: hook_config.render(HarnessHookConfigRenderInput {
                    source_path: &config.source_path,
                    target,
                    groups: &groups,
                })?,
                actual: None,
            });
            continue;
        };

        let projection = hook_config.check_projection(HarnessHookConfigCheckInput {
            path: &file_path,
            source_path: &config.source_path,
            target,
            groups: &groups,
            owned_command_prefixes: &owned_command_prefixes,
            actual_content,
        })?;
        if projection.actual != projection.expected {
            mismatches.push(HarnessHookConfigMismatch {
                path: target.path.clone(),
                expected: projection.expected,
                actual: Some(projection.actual),
            });
        }
    }
    Ok(HarnessHookConfigCheckResult {
        ok: mismatches.is_empty(),
        mismatches,
    })
}

pub fn write_harness_hook_config_files(
    root: &Path,
    config: &HarnessHookConfigSet,
) -> Result<Vec<HarnessHookFile>, HarnessHookConfigError> {
    validate_harness_hook_config_set(config)?;
    let mut files = Vec::new();
    for target in &config.targets {
        let hook_config = hook_config_for_target(config, target)?;
        let groups = harness_hook_groups(config, target);
        let owned_command_prefixes = owned_command_prefixes(config, target);
        let file_path = root.join(&target.path);
        let existing_content = read_optional_to_string(&file_path)?;
        let content = hook_config.write_content(HarnessHookConfigWriteInput {
            path: &file_path,
            source_path: &config.source_path,
            target,
            groups: &groups,
            owned_command_prefixes: &owned_command_prefixes,
            existing_content: existing_content.as_deref(),
        })?;
        files.push(HarnessHookFile {
            path: target.path.clone(),
            content,
        });
    }

    for file in &files {
        let file_path = root.join(&file.path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| HarnessHookConfigError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(&file_path, &file.content).map_err(|source| HarnessHookConfigError::Io {
            path: file_path,
            source,
        })?;
    }
    Ok(files)
}

pub fn validate_harness_hook_config_set(
    config: &HarnessHookConfigSet,
) -> Result<(), HarnessHookConfigError> {
    let mut targets = BTreeMap::new();
    let mut hooks = HashSet::new();

    for target in &config.targets {
        let hook_config = config.hook_configs.get(&target.harness).ok_or_else(|| {
            HarnessHookConfigError::MissingHarnessConfig {
                harness: target.harness.clone(),
            }
        })?;
        if target.harness.is_empty() {
            return Err(HarnessHookConfigError::EmptyTargetId);
        }
        if target.path.is_empty() {
            return Err(HarnessHookConfigError::EmptyTargetPath {
                harness: target.harness.clone(),
            });
        }
        if target.command_prefix.is_empty() {
            return Err(HarnessHookConfigError::EmptyTargetCommandPrefix {
                harness: target.harness.clone(),
            });
        }
        if targets.contains_key(&target.harness) {
            return Err(HarnessHookConfigError::DuplicateHarnessTarget {
                harness: target.harness.clone(),
            });
        }
        hook_config.validate_target(target)?;
        targets.insert(target.harness.clone(), hook_config);
    }

    for hook in &config.hooks {
        if hook.name.is_empty() {
            return Err(HarnessHookConfigError::EmptyHookCommandName);
        }
        if !hooks.insert(hook.name.as_str()) {
            return Err(HarnessHookConfigError::DuplicateHookCommand {
                command: hook.name.clone(),
            });
        }
        for binding in &hook.bindings {
            let hook_config = targets.get(&binding.harness).ok_or_else(|| {
                HarnessHookConfigError::UnknownBindingHarness {
                    command: hook.name.clone(),
                    harness: binding.harness.clone(),
                }
            })?;
            if binding.timeout == 0 {
                return Err(HarnessHookConfigError::NonPositiveTimeout {
                    command: hook.name.clone(),
                });
            }
            hook_config.validate_binding(HookBindingValidationInput { hook, binding })?;
        }
    }

    Ok(())
}

pub fn codex_harness_hook_config() -> HarnessHookConfig {
    define_harness_hook_config(HarnessHookConfig::new(
        ".codex/config.toml",
        render_codex_toml,
    ))
    .with_write(write_codex_toml)
    .with_check(check_codex_toml)
}

pub fn claude_harness_hook_config() -> HarnessHookConfig {
    define_harness_hook_config(HarnessHookConfig::new(
        ".claude/settings.json",
        render_claude_json,
    ))
    .with_write(write_claude_json)
    .with_check(check_claude_json)
}

pub fn hook_groups_by_stage(groups: &[HarnessHookGroup]) -> Value {
    let mut hooks = Map::new();
    for group in groups {
        let stage_hooks = hooks
            .entry(group.stage.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(stage_hooks) = stage_hooks {
            let mut group_json = Map::new();
            if let Some(matcher) = &group.matcher {
                group_json.insert("matcher".to_owned(), json!(matcher));
            }
            group_json.insert(
                "hooks".to_owned(),
                Value::Array(
                    group
                        .hooks
                        .iter()
                        .map(renderable_harness_hook_command)
                        .collect(),
                ),
            );
            stage_hooks.push(Value::Object(group_json));
        }
    }
    Value::Object(hooks)
}

fn render_harness_hook_config_file(
    config: &HarnessHookConfigSet,
    target: &HarnessTarget,
) -> Result<String, HarnessHookConfigError> {
    let hook_config = hook_config_for_target(config, target)?;
    let groups = harness_hook_groups(config, target);
    hook_config.render(HarnessHookConfigRenderInput {
        source_path: &config.source_path,
        target,
        groups: &groups,
    })
}

fn hook_config_for_target<'a>(
    config: &'a HarnessHookConfigSet,
    target: &HarnessTarget,
) -> Result<&'a HarnessHookConfig, HarnessHookConfigError> {
    config.hook_configs.get(&target.harness).ok_or_else(|| {
        HarnessHookConfigError::MissingHarnessConfig {
            harness: target.harness.clone(),
        }
    })
}

fn harness_hook_groups(
    config: &HarnessHookConfigSet,
    target: &HarnessTarget,
) -> Vec<HarnessHookGroup> {
    let mut groups = Vec::new();
    let mut by_key = BTreeMap::new();

    for hook in &config.hooks {
        for binding in &hook.bindings {
            if binding.harness != target.harness {
                continue;
            }

            let stage = harness_hook_stage_name(hook.stage).to_owned();
            let key = format!("{}\0{}", stage, binding.matcher.as_deref().unwrap_or(""));
            let index = match by_key.get(&key) {
                Some(index) => *index,
                None => {
                    let index = groups.len();
                    groups.push(HarnessHookGroup {
                        stage: stage.clone(),
                        matcher: binding.matcher.clone(),
                        hooks: Vec::new(),
                    });
                    by_key.insert(key, index);
                    index
                }
            };

            groups[index].hooks.push(HarnessHookCommand {
                command_type: "command".to_owned(),
                command: format!(
                    "{} {}",
                    binding
                        .command_prefix
                        .as_deref()
                        .unwrap_or(&target.command_prefix),
                    hook.name
                ),
                timeout: binding.timeout,
                status_message: binding.status_message.clone(),
                binding_options: binding.options.clone(),
            });
        }
    }

    groups
}

fn owned_command_prefixes(config: &HarnessHookConfigSet, target: &HarnessTarget) -> Vec<String> {
    let mut prefixes = BTreeMap::from([(target.command_prefix.clone(), ())]);
    for hook in &config.hooks {
        for binding in &hook.bindings {
            if binding.harness == target.harness
                && let Some(prefix) = &binding.command_prefix
            {
                prefixes.insert(prefix.clone(), ());
            }
        }
    }
    prefixes.into_keys().collect()
}

/// Runweaver-owned hook commands start with an active command prefix followed by a space or end.
fn is_owned_hook_command(command: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| {
        command
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(' '))
    })
}

fn harness_hook_stage_name(stage: HookStage) -> &'static str {
    match stage {
        HookStage::PreTool => "PreToolUse",
        HookStage::PostEdit => "PostToolUse",
        HookStage::Stop => "Stop",
    }
}

fn render_codex_toml(
    input: HarnessHookConfigRenderInput<'_>,
) -> Result<String, HarnessHookConfigError> {
    let mut sections = Vec::new();
    if let Some(Value::Object(features)) = input.target.options.get("features") {
        sections.push(render_toml_features(features));
    }
    sections.extend(input.groups.iter().map(render_toml_hook_group));
    Ok(format!("{}\n", sections.join("\n\n")))
}

fn render_claude_json(
    input: HarnessHookConfigRenderInput<'_>,
) -> Result<String, HarnessHookConfigError> {
    json_to_pretty_string(&expected_claude_json_subset(input.target, input.groups))
}

fn renderable_harness_hook_command(hook: &HarnessHookCommand) -> Value {
    json!({
        "type": hook.command_type,
        "command": hook.command,
        "timeout": hook.timeout,
        "statusMessage": hook.status_message,
    })
}

fn render_toml_features(features: &Map<String, Value>) -> String {
    let mut lines = vec!["[features]".to_owned()];
    lines.extend(
        features
            .iter()
            .filter_map(|(key, value)| value.as_bool().map(|value| format!("{key} = {value}"))),
    );
    lines.join("\n").trim_end().to_owned()
}

fn render_toml_hook_group(group: &HarnessHookGroup) -> String {
    let mut lines = vec![format!("[[hooks.{}]]", group.stage)];
    if let Some(matcher) = &group.matcher {
        lines.push(format!("matcher = {}", toml_basic_string(matcher)));
    }

    for hook in &group.hooks {
        lines.push(String::new());
        lines.push(format!("[[hooks.{}.hooks]]", group.stage));
        lines.push(format!("type = {}", toml_basic_string(&hook.command_type)));
        lines.push(format!(
            "command = {}",
            toml_literal_or_basic_string(&hook.command)
        ));
        lines.push(format!("timeout = {}", hook.timeout));
        lines.push(format!(
            "statusMessage = {}",
            toml_basic_string(&hook.status_message)
        ));
    }

    lines.join("\n")
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn toml_literal_or_basic_string(value: &str) -> String {
    if value.contains('\'') {
        toml_basic_string(value)
    } else {
        format!("'{value}'")
    }
}

fn write_claude_json(
    input: HarnessHookConfigWriteInput<'_>,
) -> Result<String, HarnessHookConfigError> {
    let mut document = match input.existing_content {
        Some(content) if !content.trim().is_empty() => parse_json_file(input.path, content)?,
        _ => Value::Object(Map::new()),
    };
    let root = json_object_mut(input.path, &mut document, "top-level settings")?;
    remove_owned_claude_hooks(input.path, root, input.owned_command_prefixes)?;
    append_generated_claude_hooks(input.path, root, input.groups)?;
    merge_claude_worktree(input.path, root, input.target)?;
    json_to_pretty_string(&document)
}

fn check_claude_json(
    input: HarnessHookConfigCheckInput<'_>,
) -> Result<HarnessHookConfigCheckProjection, HarnessHookConfigError> {
    let actual_document = parse_json_file(input.path, input.actual_content)?;
    let expected = expected_claude_json_subset(input.target, input.groups);
    let actual = actual_claude_json_subset(
        input.path,
        &actual_document,
        input.owned_command_prefixes,
        input.target,
    )?;
    Ok(HarnessHookConfigCheckProjection {
        expected: json_to_pretty_string(&expected)?,
        actual: json_to_pretty_string(&actual)?,
    })
}

fn expected_claude_json_subset(target: &HarnessTarget, groups: &[HarnessHookGroup]) -> Value {
    let mut root = Map::new();
    root.insert("hooks".to_owned(), hook_groups_by_stage(groups));
    if let Some(Value::Array(directories)) = target.options.get("worktreeSymlinkDirectories") {
        let mut worktree = Map::new();
        worktree.insert(
            "symlinkDirectories".to_owned(),
            Value::Array(directories.clone()),
        );
        root.insert("worktree".to_owned(), Value::Object(worktree));
    }
    Value::Object(root)
}

fn actual_claude_json_subset(
    path: &Path,
    document: &Value,
    prefixes: &[String],
    target: &HarnessTarget,
) -> Result<Value, HarnessHookConfigError> {
    let root = json_object(path, document, "top-level settings")?;
    let mut subset = Map::new();
    if let Some(hooks) = root.get("hooks") {
        let hook_subset = owned_claude_hooks_subset(path, hooks, prefixes)?;
        if !hook_subset.is_empty() {
            subset.insert("hooks".to_owned(), Value::Object(hook_subset));
        }
    }
    if target.options.contains_key("worktreeSymlinkDirectories")
        && let Some(symlink_directories) = root
            .get("worktree")
            .and_then(Value::as_object)
            .and_then(|worktree| worktree.get("symlinkDirectories"))
    {
        let mut worktree = Map::new();
        worktree.insert("symlinkDirectories".to_owned(), symlink_directories.clone());
        subset.insert("worktree".to_owned(), Value::Object(worktree));
    }
    Ok(Value::Object(subset))
}

fn remove_owned_claude_hooks(
    path: &Path,
    root: &mut Map<String, Value>,
    prefixes: &[String],
) -> Result<(), HarnessHookConfigError> {
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = json_object_mut(path, hooks, "hooks")?;
    let mut empty_stages = Vec::new();
    for (stage, groups) in hooks.iter_mut() {
        let groups = json_array_mut(path, groups, "hook stage")?;
        groups.retain_mut(|group| {
            let Some(group) = group.as_object_mut() else {
                return true;
            };
            let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            hooks.retain(|hook| {
                hook.get("command")
                    .and_then(Value::as_str)
                    .is_none_or(|command| !is_owned_hook_command(command, prefixes))
            });
            !hooks.is_empty()
        });
        if groups.is_empty() {
            empty_stages.push(stage.clone());
        }
    }
    for stage in empty_stages {
        hooks.remove(&stage);
    }
    Ok(())
}

fn append_generated_claude_hooks(
    path: &Path,
    root: &mut Map<String, Value>,
    groups: &[HarnessHookGroup],
) -> Result<(), HarnessHookConfigError> {
    let generated = hook_groups_by_stage(groups);
    let generated = generated
        .as_object()
        .ok_or_else(|| HarnessHookConfigError::InvalidShape {
            path: path.to_path_buf(),
            message: "generated hooks must be an object".to_owned(),
        })?;
    let hooks = root
        .entry("hooks".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = json_object_mut(path, hooks, "hooks")?;
    for (stage, generated_groups) in generated {
        let stage_groups = hooks
            .entry(stage.clone())
            .or_insert_with(|| Value::Array(Vec::new()));
        let stage_groups = json_array_mut(path, stage_groups, "hook stage")?;
        let generated_groups =
            generated_groups
                .as_array()
                .ok_or_else(|| HarnessHookConfigError::InvalidShape {
                    path: path.to_path_buf(),
                    message: format!("generated hook stage {stage} must be an array"),
                })?;
        stage_groups.extend(generated_groups.iter().cloned());
    }
    Ok(())
}

fn merge_claude_worktree(
    path: &Path,
    root: &mut Map<String, Value>,
    target: &HarnessTarget,
) -> Result<(), HarnessHookConfigError> {
    let Some(Value::Array(directories)) = target.options.get("worktreeSymlinkDirectories") else {
        return Ok(());
    };
    let worktree = root
        .entry("worktree".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let worktree = json_object_mut(path, worktree, "worktree")?;
    worktree.insert(
        "symlinkDirectories".to_owned(),
        Value::Array(directories.clone()),
    );
    Ok(())
}

fn owned_claude_hooks_subset(
    path: &Path,
    hooks: &Value,
    prefixes: &[String],
) -> Result<Map<String, Value>, HarnessHookConfigError> {
    let hooks = json_object(path, hooks, "hooks")?;
    let mut hook_subset = Map::new();
    for (stage, groups) in hooks {
        let groups = json_array(path, groups, "hook stage")?;
        let mut owned_groups = Vec::new();
        for group in groups {
            let group = json_object(path, group, "hook group")?;
            let Some(hooks) = group.get("hooks") else {
                continue;
            };
            let owned_hooks = json_array(path, hooks, "hook commands")?
                .iter()
                .filter(|hook| {
                    hook.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|command| is_owned_hook_command(command, prefixes))
                })
                .cloned()
                .collect::<Vec<_>>();
            if owned_hooks.is_empty() {
                continue;
            }
            let mut owned_group = Map::new();
            if let Some(matcher) = group.get("matcher") {
                owned_group.insert("matcher".to_owned(), matcher.clone());
            }
            owned_group.insert("hooks".to_owned(), Value::Array(owned_hooks));
            owned_groups.push(Value::Object(owned_group));
        }
        if !owned_groups.is_empty() {
            hook_subset.insert(stage.clone(), Value::Array(owned_groups));
        }
    }
    Ok(hook_subset)
}

fn parse_json_file(path: &Path, content: &str) -> Result<Value, HarnessHookConfigError> {
    serde_json::from_str(content).map_err(|source| HarnessHookConfigError::InvalidJson {
        path: path.to_path_buf(),
        source,
    })
}

fn json_object<'a>(
    path: &Path,
    value: &'a Value,
    label: &str,
) -> Result<&'a Map<String, Value>, HarnessHookConfigError> {
    value
        .as_object()
        .ok_or_else(|| HarnessHookConfigError::InvalidShape {
            path: path.to_path_buf(),
            message: format!("{label} must be an object"),
        })
}

fn json_object_mut<'a>(
    path: &Path,
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut Map<String, Value>, HarnessHookConfigError> {
    value
        .as_object_mut()
        .ok_or_else(|| HarnessHookConfigError::InvalidShape {
            path: path.to_path_buf(),
            message: format!("{label} must be an object"),
        })
}

fn json_array<'a>(
    path: &Path,
    value: &'a Value,
    label: &str,
) -> Result<&'a Vec<Value>, HarnessHookConfigError> {
    value
        .as_array()
        .ok_or_else(|| HarnessHookConfigError::InvalidShape {
            path: path.to_path_buf(),
            message: format!("{label} must be an array"),
        })
}

fn json_array_mut<'a>(
    path: &Path,
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut Vec<Value>, HarnessHookConfigError> {
    value
        .as_array_mut()
        .ok_or_else(|| HarnessHookConfigError::InvalidShape {
            path: path.to_path_buf(),
            message: format!("{label} must be an array"),
        })
}

fn json_to_pretty_string(value: &Value) -> Result<String, HarnessHookConfigError> {
    serde_json::to_string_pretty(value)
        .map(|content| format!("{content}\n"))
        .map_err(|error| HarnessHookConfigError::Custom {
            message: format!("Failed to render JSON: {error}"),
        })
}

fn write_codex_toml(
    input: HarnessHookConfigWriteInput<'_>,
) -> Result<String, HarnessHookConfigError> {
    let Some(existing_content) = input.existing_content else {
        return render_codex_toml(HarnessHookConfigRenderInput {
            source_path: input.source_path,
            target: input.target,
            groups: input.groups,
        });
    };
    if existing_content.trim().is_empty() {
        return render_codex_toml(HarnessHookConfigRenderInput {
            source_path: input.source_path,
            target: input.target,
            groups: input.groups,
        });
    }

    let mut document = parse_toml_file(input.path, existing_content)?;
    remove_owned_codex_hooks(&mut document, input.owned_command_prefixes);
    merge_codex_features(&mut document, input.target)?;
    append_generated_codex_hooks(&mut document, input.groups)?;
    Ok(document.to_string())
}

fn check_codex_toml(
    input: HarnessHookConfigCheckInput<'_>,
) -> Result<HarnessHookConfigCheckProjection, HarnessHookConfigError> {
    let actual_document = parse_toml_file(input.path, input.actual_content)?;
    let expected = expected_codex_toml_subset(input.target, input.groups)?;
    let actual =
        actual_codex_toml_subset(&actual_document, input.owned_command_prefixes, input.target)?;
    Ok(HarnessHookConfigCheckProjection { expected, actual })
}

fn expected_codex_toml_subset(
    target: &HarnessTarget,
    groups: &[HarnessHookGroup],
) -> Result<String, HarnessHookConfigError> {
    render_codex_toml(HarnessHookConfigRenderInput {
        source_path: "",
        target,
        groups,
    })
}

fn actual_codex_toml_subset(
    document: &DocumentMut,
    prefixes: &[String],
    target: &HarnessTarget,
) -> Result<String, HarnessHookConfigError> {
    let mut subset = DocumentMut::new();
    copy_owned_codex_features(document, &mut subset, target)?;
    copy_owned_codex_hooks(document, &mut subset, prefixes)?;
    Ok(subset.to_string())
}

fn parse_toml_file(path: &Path, content: &str) -> Result<DocumentMut, HarnessHookConfigError> {
    content
        .parse::<DocumentMut>()
        .map_err(|source| HarnessHookConfigError::InvalidToml {
            path: path.to_path_buf(),
            source,
        })
}

fn remove_owned_codex_hooks(document: &mut DocumentMut, prefixes: &[String]) {
    let Some(hooks) = document.get_mut("hooks").and_then(Item::as_table_mut) else {
        return;
    };
    let stages = hooks
        .iter()
        .filter(|(_, item)| item.is_array_of_tables())
        .map(|(stage, _)| stage.to_owned())
        .collect::<Vec<_>>();
    for stage in stages {
        let remove_stage = hooks
            .get_mut(&stage)
            .and_then(Item::as_array_of_tables_mut)
            .is_some_and(|stage_groups| {
                prune_owned_toml_stage(stage_groups, prefixes);
                stage_groups.is_empty()
            });
        if remove_stage {
            hooks.remove(&stage);
        }
    }
}

fn prune_owned_toml_stage(stage_groups: &mut ArrayOfTables, prefixes: &[String]) {
    for group in stage_groups.iter_mut() {
        if let Some(hooks) = group
            .get_mut("hooks")
            .and_then(Item::as_array_of_tables_mut)
        {
            hooks.retain(|hook| {
                hook.get("command")
                    .and_then(Item::as_str)
                    .is_none_or(|command| !is_owned_hook_command(command, prefixes))
            });
        }
    }
    stage_groups.retain(|group| {
        group
            .get("hooks")
            .and_then(Item::as_array_of_tables)
            .is_some_and(|hooks| !hooks.is_empty())
    });
}

fn merge_codex_features(
    document: &mut DocumentMut,
    target: &HarnessTarget,
) -> Result<(), HarnessHookConfigError> {
    let Some(Value::Object(features)) = target.options.get("features") else {
        return Ok(());
    };
    let features_table = ensure_toml_table(document.as_table_mut(), "features")?;
    for (key, feature) in features {
        if let Some(feature) = feature.as_bool() {
            features_table.insert(key, value(feature));
        }
    }
    Ok(())
}

fn append_generated_codex_hooks(
    document: &mut DocumentMut,
    groups: &[HarnessHookGroup],
) -> Result<(), HarnessHookConfigError> {
    let hooks_table = ensure_toml_implicit_table(document.as_table_mut(), "hooks")?;
    for group in groups {
        let mut group_table = Table::new();
        if let Some(matcher) = &group.matcher {
            group_table.insert("matcher", value(matcher.clone()));
        }

        let mut hook_tables = ArrayOfTables::new();
        for hook in &group.hooks {
            hook_tables.push(codex_hook_table(hook));
        }
        group_table.insert("hooks", Item::ArrayOfTables(hook_tables));

        ensure_toml_array_of_tables(hooks_table, &group.stage)?.push(group_table);
    }
    Ok(())
}

fn copy_owned_codex_features(
    source: &DocumentMut,
    target: &mut DocumentMut,
    harness_target: &HarnessTarget,
) -> Result<(), HarnessHookConfigError> {
    let Some(Value::Object(features)) = harness_target.options.get("features") else {
        return Ok(());
    };
    for key in features.keys() {
        if let Some(feature) = source
            .get("features")
            .and_then(Item::as_table)
            .and_then(|features| features.get(key))
            .and_then(Item::as_bool)
        {
            ensure_toml_table(target.as_table_mut(), "features")?.insert(key, value(feature));
        }
    }
    Ok(())
}

fn copy_owned_codex_hooks(
    source: &DocumentMut,
    target: &mut DocumentMut,
    prefixes: &[String],
) -> Result<(), HarnessHookConfigError> {
    let Some(hooks) = source.get("hooks").and_then(Item::as_table) else {
        return Ok(());
    };
    let target_hooks = ensure_toml_implicit_table(target.as_table_mut(), "hooks")?;
    for (stage, stage_groups) in hooks {
        let Some(stage_groups) = stage_groups.as_array_of_tables() else {
            continue;
        };
        for group in stage_groups.iter() {
            let Some(hooks) = group.get("hooks").and_then(Item::as_array_of_tables) else {
                continue;
            };
            let mut owned_hooks = ArrayOfTables::new();
            for hook in hooks.iter() {
                if hook
                    .get("command")
                    .and_then(Item::as_str)
                    .is_some_and(|command| is_owned_hook_command(command, prefixes))
                {
                    owned_hooks.push(hook.clone());
                }
            }
            if owned_hooks.is_empty() {
                continue;
            }

            let mut group_table = Table::new();
            if let Some(matcher) = group.get("matcher").and_then(Item::as_str) {
                group_table.insert("matcher", value(matcher));
            }
            group_table.insert("hooks", Item::ArrayOfTables(owned_hooks));
            ensure_toml_array_of_tables(target_hooks, stage)?.push(group_table);
        }
    }
    Ok(())
}

fn ensure_toml_table<'a>(
    table: &'a mut Table,
    key: &str,
) -> Result<&'a mut Table, HarnessHookConfigError> {
    if !table.get(key).is_some_and(Item::is_table) {
        table.insert(key, Item::Table(Table::new()));
    }
    table
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| HarnessHookConfigError::Custom {
            message: format!("Failed to create TOML table {key}"),
        })
}

fn ensure_toml_implicit_table<'a>(
    table: &'a mut Table,
    key: &str,
) -> Result<&'a mut Table, HarnessHookConfigError> {
    let created = table.get(key).is_none();
    let table = ensure_toml_table(table, key)?;
    if created {
        table.set_implicit(true);
    }
    Ok(table)
}

fn ensure_toml_array_of_tables<'a>(
    table: &'a mut Table,
    key: &str,
) -> Result<&'a mut ArrayOfTables, HarnessHookConfigError> {
    if !table.get(key).is_some_and(Item::is_array_of_tables) {
        table.insert(key, Item::ArrayOfTables(ArrayOfTables::new()));
    }
    table
        .get_mut(key)
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| HarnessHookConfigError::Custom {
            message: format!("Failed to create TOML array of tables {key}"),
        })
}

fn codex_hook_table(hook: &HarnessHookCommand) -> Table {
    let mut hook_table = Table::new();
    hook_table.insert("type", value(hook.command_type.clone()));
    hook_table.insert(
        "command",
        toml_literal_or_basic_string(&hook.command)
            .parse::<Item>()
            .unwrap_or_else(|_| value(hook.command.clone())),
    );
    hook_table.insert("timeout", value(i64::from(hook.timeout)));
    hook_table.insert("statusMessage", value(hook.status_message.clone()));
    hook_table
}

fn read_optional_to_string(path: &Path) -> Result<Option<String>, HarnessHookConfigError> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(HarnessHookConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_harness_hook_config() -> HarnessHookConfig {
        define_harness_hook_config(HarnessHookConfig::new(".fixture/hooks.json", |input| {
            let groups = Value::Array(
                input
                    .groups
                    .iter()
                    .map(|group| {
                        let mut group_json = Map::new();
                        group_json.insert("stage".to_owned(), json!(group.stage));
                        if let Some(matcher) = &group.matcher {
                            group_json.insert("matcher".to_owned(), json!(matcher));
                        }
                        group_json.insert(
                            "hooks".to_owned(),
                            Value::Array(
                                group
                                    .hooks
                                    .iter()
                                    .map(|hook| {
                                        let mut hook_json = Map::new();
                                        hook_json.insert("command".to_owned(), json!(hook.command));
                                        hook_json.insert("timeout".to_owned(), json!(hook.timeout));
                                        hook_json.insert(
                                            "statusMessage".to_owned(),
                                            json!(hook.status_message),
                                        );
                                        for (key, value) in &hook.binding_options {
                                            hook_json.insert(key.clone(), value.clone());
                                        }
                                        Value::Object(hook_json)
                                    })
                                    .collect(),
                            ),
                        );
                        Value::Object(group_json)
                    })
                    .collect(),
            );
            let content = json!({
                "sourcePath": input.source_path,
                "groups": groups,
            });
            Ok(format!(
                "{}\n",
                serde_json::to_string_pretty(&content).unwrap()
            ))
        }))
    }

    fn hook_config_set() -> HarnessHookConfigSet {
        HarnessHookConfigSet {
            source_path: "agent-hooks.config.ts".to_owned(),
            hook_configs: HarnessHookConfigRegistry::from([
                ("fixture".to_owned(), fixture_harness_hook_config()),
                ("codex".to_owned(), codex_harness_hook_config()),
                ("claude".to_owned(), claude_harness_hook_config()),
            ]),
            targets: vec![
                HarnessTarget::new("fixture", ".fixture/hooks.json", "agent-hooks fixture"),
                HarnessTarget::new("codex", ".codex/config.toml", "agent-hooks codex")
                    .with_option("features", json!({ "hooks": true })),
                HarnessTarget::new("claude", ".claude/settings.json", "agent-hooks claude")
                    .with_option("worktreeSymlinkDirectories", json!(["node_modules"])),
            ],
            hooks: vec![
                HookConfigCommand::new(
                    "guard-example",
                    HookStage::PreTool,
                    vec![
                        HookBinding::new("fixture", 10, "Check Fixture")
                            .with_matcher("Bash")
                            .with_option("mode", json!("changed-files")),
                        HookBinding::new("codex", 10, "Check Codex").with_matcher("^Bash$"),
                        HookBinding::new("claude", 10, "Check Claude").with_matcher("Bash"),
                    ],
                ),
                HookConfigCommand::new(
                    "summarize-session",
                    HookStage::Stop,
                    vec![
                        HookBinding::new("codex", 5, "Summarize Codex"),
                        HookBinding::new("claude", 5, "Summarize Claude"),
                    ],
                ),
            ],
        }
    }

    fn single_harness_config(harness: &str) -> HarnessHookConfigSet {
        let config = hook_config_set();
        let target = config
            .targets
            .iter()
            .find(|target| target.harness == harness)
            .unwrap()
            .clone();
        let hook_config = config.hook_configs.get(harness).unwrap().clone();
        let hooks = config
            .hooks
            .iter()
            .filter_map(|hook| {
                let bindings = hook
                    .bindings
                    .iter()
                    .filter(|binding| binding.harness == harness)
                    .cloned()
                    .collect::<Vec<_>>();
                if bindings.is_empty() {
                    None
                } else {
                    Some(HookConfigCommand::new(
                        hook.name.clone(),
                        hook.stage,
                        bindings,
                    ))
                }
            })
            .collect();

        HarnessHookConfigSet {
            source_path: config.source_path,
            hook_configs: HarnessHookConfigRegistry::from([(harness.to_owned(), hook_config)]),
            targets: vec![target],
            hooks,
        }
    }

    #[test]
    fn renders_custom_codex_and_claude_native_hook_config_files() {
        let files = render_harness_hook_config_files(&hook_config_set()).unwrap();

        assert_eq!(
            files[0],
            HarnessHookFile {
                path: ".fixture/hooks.json".to_owned(),
                content: "{\n  \"sourcePath\": \"agent-hooks.config.ts\",\n  \"groups\": [\n    {\n      \"stage\": \"PreToolUse\",\n      \"matcher\": \"Bash\",\n      \"hooks\": [\n        {\n          \"command\": \"agent-hooks fixture guard-example\",\n          \"timeout\": 10,\n          \"statusMessage\": \"Check Fixture\",\n          \"mode\": \"changed-files\"\n        }\n      ]\n    }\n  ]\n}\n".to_owned(),
            }
        );
        assert_eq!(
            files[1],
            HarnessHookFile {
                path: ".codex/config.toml".to_owned(),
                content: "[features]\nhooks = true\n\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n\n[[hooks.PreToolUse.hooks]]\ntype = \"command\"\ncommand = 'agent-hooks codex guard-example'\ntimeout = 10\nstatusMessage = \"Check Codex\"\n\n[[hooks.Stop]]\n\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = 'agent-hooks codex summarize-session'\ntimeout = 5\nstatusMessage = \"Summarize Codex\"\n".to_owned(),
            }
        );
        assert_eq!(
            files[2],
            HarnessHookFile {
                path: ".claude/settings.json".to_owned(),
                content: "{\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"matcher\": \"Bash\",\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"agent-hooks claude guard-example\",\n            \"timeout\": 10,\n            \"statusMessage\": \"Check Claude\"\n          }\n        ]\n      }\n    ],\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"agent-hooks claude summarize-session\",\n            \"timeout\": 5,\n            \"statusMessage\": \"Summarize Claude\"\n          }\n        ]\n      }\n    ]\n  },\n  \"worktree\": {\n    \"symlinkDirectories\": [\n      \"node_modules\"\n    ]\n  }\n}\n".to_owned(),
            }
        );
    }

    #[test]
    fn check_detects_drift_and_write_restores_generated_files() {
        let root = test_root("harness-config");
        let config = hook_config_set();

        for file in render_harness_hook_config_files(&config).unwrap() {
            let drift = match file.path.as_str() {
                ".codex/config.toml" => "model = \"drift\"\n",
                ".claude/settings.json" => "{}\n",
                _ => "drift\n",
            };
            write_fixture_file(&root, &file.path, drift);
        }

        assert!(!check_harness_hook_config_files(&root, &config).unwrap().ok);
        write_harness_hook_config_files(&root, &config).unwrap();
        assert_eq!(
            check_harness_hook_config_files(&root, &config).unwrap(),
            HarnessHookConfigCheckResult {
                ok: true,
                mismatches: Vec::new(),
            }
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unknown_harness_duplicate_targets_and_invalid_timeouts() {
        let config = hook_config_set();
        let unknown_target = HarnessHookConfigSet {
            targets: vec![HarnessTarget::new(
                "unknown",
                ".unknown/config",
                "agent-hooks",
            )],
            ..config.clone()
        };
        assert_eq!(
            render_harness_hook_config_files(&unknown_target)
                .unwrap_err()
                .to_string(),
            "Harness target unknown does not define hook-config support."
        );

        let duplicate_target = HarnessHookConfigSet {
            targets: vec![config.targets[1].clone(), config.targets[1].clone()],
            ..config.clone()
        };
        assert_eq!(
            render_harness_hook_config_files(&duplicate_target)
                .unwrap_err()
                .to_string(),
            "Duplicate harness target: codex"
        );

        let invalid_timeout = HarnessHookConfigSet {
            hooks: vec![HookConfigCommand::new(
                "invalid-timeout",
                HookStage::PreTool,
                vec![HookBinding::new("codex", 0, "Invalid")],
            )],
            ..config
        };
        assert_eq!(
            render_harness_hook_config_files(&invalid_timeout)
                .unwrap_err()
                .to_string(),
            "Hook command invalid-timeout must define a positive timeout."
        );
    }

    #[test]
    fn groups_hooks_by_stage_for_claude_shape() {
        let groups = vec![
            HarnessHookGroup {
                stage: "PreToolUse".to_owned(),
                matcher: Some("Bash".to_owned()),
                hooks: vec![HarnessHookCommand {
                    command_type: "command".to_owned(),
                    command: "agent-hooks claude guard-example".to_owned(),
                    timeout: 10,
                    status_message: "Check Claude".to_owned(),
                    binding_options: HarnessOptions::new(),
                }],
            },
            HarnessHookGroup {
                stage: "Stop".to_owned(),
                matcher: None,
                hooks: vec![HarnessHookCommand {
                    command_type: "command".to_owned(),
                    command: "agent-hooks claude summarize-session".to_owned(),
                    timeout: 5,
                    status_message: "Summarize Claude".to_owned(),
                    binding_options: HarnessOptions::new(),
                }],
            },
        ];

        assert_eq!(
            hook_groups_by_stage(&groups),
            json!({
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "agent-hooks claude guard-example",
                        "timeout": 10,
                        "statusMessage": "Check Claude"
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "agent-hooks claude summarize-session",
                        "timeout": 5,
                        "statusMessage": "Summarize Claude"
                    }]
                }]
            })
        );
    }

    #[test]
    fn write_merges_claude_json_without_dropping_foreign_content() {
        let root = test_root("claude-json-merge-foreign");
        let config = single_harness_config("claude");
        write_fixture_file(
            &root,
            ".claude/settings.json",
            r#"{
  "permissions": {
    "allow": ["Bash(git status)"]
  },
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "./scripts/my-hook.sh",
            "timeout": 3,
            "statusMessage": "Mine"
          }
        ]
      }
    ]
  }
}
"#,
        );

        write_harness_hook_config_files(&root, &config).unwrap();
        let settings = read_json_fixture(&root, ".claude/settings.json");
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(settings["permissions"]["allow"][0], "Bash(git status)");
        assert!(value_contains_string(
            &settings["hooks"],
            "./scripts/my-hook.sh"
        ));
        assert!(value_contains_string(
            &settings["hooks"],
            "agent-hooks claude guard-example"
        ));
    }

    #[test]
    fn write_replaces_stale_owned_claude_json_entries() {
        let root = test_root("claude-json-replace-owned");
        let config = single_harness_config("claude");
        write_fixture_file(
            &root,
            ".claude/settings.json",
            r#"{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "./scripts/my-hook.sh",
            "timeout": 3,
            "statusMessage": "Mine"
          },
          {
            "type": "command",
            "command": "agent-hooks claude stale-hook",
            "timeout": 99,
            "statusMessage": "Stale"
          }
        ]
      }
    ]
  }
}
"#,
        );

        write_harness_hook_config_files(&root, &config).unwrap();
        let settings = read_json_fixture(&root, ".claude/settings.json");
        std::fs::remove_dir_all(root).unwrap();

        assert!(value_contains_string(
            &settings["hooks"],
            "./scripts/my-hook.sh"
        ));
        assert!(!value_contains_string(
            &settings["hooks"],
            "agent-hooks claude stale-hook"
        ));
        assert!(value_contains_string(
            &settings["hooks"],
            "agent-hooks claude guard-example"
        ));
    }

    #[test]
    fn write_is_idempotent_for_claude_json_and_codex_toml() {
        let root = test_root("hook-config-idempotent");
        let config = hook_config_set();

        write_harness_hook_config_files(&root, &config).unwrap();
        let first_claude = read_fixture_file(&root, ".claude/settings.json");
        let first_codex = read_fixture_file(&root, ".codex/config.toml");
        write_harness_hook_config_files(&root, &config).unwrap();
        let second_claude = read_fixture_file(&root, ".claude/settings.json");
        let second_codex = read_fixture_file(&root, ".codex/config.toml");
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(second_claude, first_claude);
        assert_eq!(second_codex, first_codex);
    }

    #[test]
    fn write_merges_codex_toml_without_dropping_comments_or_foreign_tables() {
        let root = test_root("codex-toml-merge-foreign");
        let config = single_harness_config("codex");
        write_fixture_file(
            &root,
            ".codex/config.toml",
            r#"# hand-authored comment
model = "x"

[tools]
enabled = true

[features]
existing = true
hooks = false

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = './scripts/my-hook.sh'
timeout = 3
statusMessage = "Mine"

[[hooks.PreToolUse.hooks]]
type = "command"
command = 'agent-hooks codex stale-hook'
timeout = 99
statusMessage = "Stale"
"#,
        );

        write_harness_hook_config_files(&root, &config).unwrap();
        let content = read_fixture_file(&root, ".codex/config.toml");
        std::fs::remove_dir_all(root).unwrap();

        assert!(content.contains("# hand-authored comment"));
        assert!(content.contains("model = \"x\""));
        assert!(content.contains("[tools]\nenabled = true"));
        assert!(content.contains("existing = true"));
        assert!(content.contains("hooks = true"));
        assert!(content.contains("command = './scripts/my-hook.sh'"));
        assert!(!content.contains("agent-hooks codex stale-hook"));
        assert!(content.contains("agent-hooks codex guard-example"));
    }

    #[test]
    fn write_fails_fast_on_unparseable_json_and_toml_without_touching_files() {
        let json_root = test_root("unparseable-json");
        let json_config = single_harness_config("claude");
        let invalid_json = "{ not json\n";
        write_fixture_file(&json_root, ".claude/settings.json", invalid_json);

        let json_error = write_harness_hook_config_files(&json_root, &json_config)
            .unwrap_err()
            .to_string();
        assert!(
            json_error.contains(".claude/settings.json"),
            "error should name JSON path: {json_error}"
        );
        assert_eq!(
            read_fixture_file(&json_root, ".claude/settings.json"),
            invalid_json
        );
        std::fs::remove_dir_all(json_root).unwrap();

        let toml_root = test_root("unparseable-toml");
        let toml_config = single_harness_config("codex");
        let invalid_toml = "model = \n";
        write_fixture_file(&toml_root, ".codex/config.toml", invalid_toml);

        let toml_error = write_harness_hook_config_files(&toml_root, &toml_config)
            .unwrap_err()
            .to_string();
        assert!(
            toml_error.contains(".codex/config.toml"),
            "error should name TOML path: {toml_error}"
        );
        assert_eq!(
            read_fixture_file(&toml_root, ".codex/config.toml"),
            invalid_toml
        );
        std::fs::remove_dir_all(toml_root).unwrap();
    }

    #[test]
    fn check_ignores_foreign_json_and_toml_content() {
        let root = test_root("check-ignores-foreign");
        let config = hook_config_set();
        write_harness_hook_config_files(&root, &config).unwrap();

        let mut settings = read_json_fixture(&root, ".claude/settings.json");
        settings["permissions"] = json!({ "allow": ["Bash(git status)"] });
        settings["hooks"]
            .as_object_mut()
            .unwrap()
            .entry("PostToolUse".to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(json!({
                "matcher": "Edit",
                "hooks": [{
                    "type": "command",
                    "command": "./scripts/my-hook.sh",
                    "timeout": 3,
                    "statusMessage": "Mine"
                }]
            }));
        write_fixture_file(
            &root,
            ".claude/settings.json",
            &(serde_json::to_string_pretty(&settings).unwrap() + "\n"),
        );

        let codex = read_fixture_file(&root, ".codex/config.toml");
        write_fixture_file(
            &root,
            ".codex/config.toml",
            &format!(
                "# hand-authored comment\nmodel = \"x\"\n\n{codex}\n[[hooks.PreToolUse]]\nmatcher = \"^Bash$\"\n\n[[hooks.PreToolUse.hooks]]\ntype = \"command\"\ncommand = './scripts/my-hook.sh'\ntimeout = 3\nstatusMessage = \"Mine\"\n"
            ),
        );

        let result = check_harness_hook_config_files(&root, &config).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(
            result,
            HarnessHookConfigCheckResult {
                ok: true,
                mismatches: Vec::new(),
            }
        );
    }

    #[test]
    fn check_flags_edited_and_missing_owned_json_entries() {
        let edited_root = test_root("check-edited-owned");
        let config = single_harness_config("claude");
        write_harness_hook_config_files(&edited_root, &config).unwrap();
        let mut edited = read_json_fixture(&edited_root, ".claude/settings.json");
        edited["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"] = json!(99);
        write_fixture_file(
            &edited_root,
            ".claude/settings.json",
            &(serde_json::to_string_pretty(&edited).unwrap() + "\n"),
        );

        let edited_result = check_harness_hook_config_files(&edited_root, &config).unwrap();
        std::fs::remove_dir_all(edited_root).unwrap();

        assert!(!edited_result.ok);
        assert_eq!(edited_result.mismatches[0].path, ".claude/settings.json");

        let missing_root = test_root("check-missing-owned");
        write_harness_hook_config_files(&missing_root, &config).unwrap();
        let mut missing = read_json_fixture(&missing_root, ".claude/settings.json");
        missing["hooks"].as_object_mut().unwrap().remove("Stop");
        write_fixture_file(
            &missing_root,
            ".claude/settings.json",
            &(serde_json::to_string_pretty(&missing).unwrap() + "\n"),
        );

        let missing_result = check_harness_hook_config_files(&missing_root, &config).unwrap();
        std::fs::remove_dir_all(missing_root).unwrap();

        assert!(!missing_result.ok);
        assert_eq!(missing_result.mismatches[0].path, ".claude/settings.json");
    }

    fn write_fixture_file(root: &Path, relative_path: &str, content: &str) {
        let file_path = root.join(relative_path);
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(file_path, content).unwrap();
    }

    fn read_fixture_file(root: &Path, relative_path: &str) -> String {
        std::fs::read_to_string(root.join(relative_path)).unwrap()
    }

    fn read_json_fixture(root: &Path, relative_path: &str) -> Value {
        serde_json::from_str(&read_fixture_file(root, relative_path)).unwrap()
    }

    fn value_contains_string(value: &Value, needle: &str) -> bool {
        match value {
            Value::String(value) => value == needle,
            Value::Array(values) => values
                .iter()
                .any(|value| value_contains_string(value, needle)),
            Value::Object(values) => values
                .values()
                .any(|value| value_contains_string(value, needle)),
            _ => false,
        }
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "runweaver-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
