use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
            validate_target: None,
            validate_binding: None,
        }
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
    let mut mismatches = Vec::new();
    for file in render_harness_hook_config_files(config)? {
        let file_path = root.join(&file.path);
        let actual = if file_path.exists() {
            Some(read_to_string(&file_path)?)
        } else {
            None
        };
        if actual.as_deref() != Some(file.content.as_str()) {
            mismatches.push(HarnessHookConfigMismatch {
                path: file.path,
                expected: file.content,
                actual,
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
    let files = render_harness_hook_config_files(config)?;
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
}

pub fn claude_harness_hook_config() -> HarnessHookConfig {
    define_harness_hook_config(HarnessHookConfig::new(
        ".claude/settings.json",
        render_claude_json,
    ))
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
    let hook_config = config.hook_configs.get(&target.harness).ok_or_else(|| {
        HarnessHookConfigError::MissingHarnessConfig {
            harness: target.harness.clone(),
        }
    })?;
    let groups = harness_hook_groups(config, target);
    hook_config.render(HarnessHookConfigRenderInput {
        source_path: &config.source_path,
        target,
        groups: &groups,
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
    let mut lines = vec!["{".to_owned(), "  \"hooks\": {".to_owned()];
    let stages = ordered_groups_by_stage(input.groups);
    for (stage_index, (stage, groups)) in stages.iter().enumerate() {
        lines.push(format!("    {}: [", json_string(stage)?));
        for (group_index, group) in groups.iter().enumerate() {
            lines.push("      {".to_owned());
            if let Some(matcher) = &group.matcher {
                lines.push(format!("        \"matcher\": {},", json_string(matcher)?));
            }
            lines.push("        \"hooks\": [".to_owned());
            for (hook_index, hook) in group.hooks.iter().enumerate() {
                lines.push("          {".to_owned());
                lines.push(format!(
                    "            \"type\": {},",
                    json_string(&hook.command_type)?
                ));
                lines.push(format!(
                    "            \"command\": {},",
                    json_string(&hook.command)?
                ));
                lines.push(format!("            \"timeout\": {},", hook.timeout));
                lines.push(format!(
                    "            \"statusMessage\": {}",
                    json_string(&hook.status_message)?
                ));
                lines.push(format!(
                    "          }}{}",
                    if hook_index + 1 == group.hooks.len() {
                        ""
                    } else {
                        ","
                    }
                ));
            }
            lines.push("        ]".to_owned());
            lines.push(format!(
                "      }}{}",
                if group_index + 1 == groups.len() {
                    ""
                } else {
                    ","
                }
            ));
        }
        lines.push(format!(
            "    ]{}",
            if stage_index + 1 == stages.len() {
                ""
            } else {
                ","
            }
        ));
    }
    let worktree_directories = match input.target.options.get("worktreeSymlinkDirectories") {
        Some(Value::Array(directories)) => Some(directories),
        _ => None,
    };
    lines.push(if worktree_directories.is_some() {
        "  },".to_owned()
    } else {
        "  }".to_owned()
    });

    if let Some(directories) = worktree_directories {
        lines.push("  \"worktree\": {".to_owned());
        let rendered_directories = directories
            .iter()
            .filter_map(|directory| directory.as_str())
            .map(json_string)
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        lines.push(format!(
            "    \"symlinkDirectories\": [{rendered_directories}]"
        ));
        lines.push("  }".to_owned());
    }

    lines.push("}".to_owned());
    Ok(format!("{}\n", lines.join("\n")))
}

fn renderable_harness_hook_command(hook: &HarnessHookCommand) -> Value {
    json!({
        "type": hook.command_type,
        "command": hook.command,
        "timeout": hook.timeout,
        "statusMessage": hook.status_message,
    })
}

fn ordered_groups_by_stage(groups: &[HarnessHookGroup]) -> Vec<(String, Vec<&HarnessHookGroup>)> {
    let mut ordered = Vec::<(String, Vec<&HarnessHookGroup>)>::new();
    for group in groups {
        match ordered.iter_mut().find(|(stage, _)| stage == &group.stage) {
            Some((_, groups)) => groups.push(group),
            None => ordered.push((group.stage.clone(), vec![group])),
        }
    }
    ordered
}

fn json_string(value: &str) -> Result<String, HarnessHookConfigError> {
    serde_json::to_string(value).map_err(|error| HarnessHookConfigError::Custom {
        message: format!("Failed to render JSON string: {error}"),
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

fn read_to_string(path: &Path) -> Result<String, HarnessHookConfigError> {
    std::fs::read_to_string(path).map_err(|source| HarnessHookConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
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

    #[test]
    fn renders_custom_codex_and_claude_native_hook_config_files() {
        let files = render_harness_hook_config_files(&hook_config_set()).unwrap();

        assert_eq!(
            files[0],
            HarnessHookFile {
                path: ".fixture/hooks.json".to_owned(),
                content: "{\n  \"groups\": [\n    {\n      \"hooks\": [\n        {\n          \"command\": \"agent-hooks fixture guard-example\",\n          \"mode\": \"changed-files\",\n          \"statusMessage\": \"Check Fixture\",\n          \"timeout\": 10\n        }\n      ],\n      \"matcher\": \"Bash\",\n      \"stage\": \"PreToolUse\"\n    }\n  ],\n  \"sourcePath\": \"agent-hooks.config.ts\"\n}\n".to_owned(),
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
                content: "{\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"matcher\": \"Bash\",\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"agent-hooks claude guard-example\",\n            \"timeout\": 10,\n            \"statusMessage\": \"Check Claude\"\n          }\n        ]\n      }\n    ],\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"agent-hooks claude summarize-session\",\n            \"timeout\": 5,\n            \"statusMessage\": \"Summarize Claude\"\n          }\n        ]\n      }\n    ]\n  },\n  \"worktree\": {\n    \"symlinkDirectories\": [\"node_modules\"]\n  }\n}\n".to_owned(),
            }
        );
    }

    #[test]
    fn check_detects_drift_and_write_restores_generated_files() {
        let root = test_root("harness-config");
        let config = hook_config_set();

        for file in render_harness_hook_config_files(&config).unwrap() {
            write_fixture_file(&root, &file.path, "drift\n");
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

    fn write_fixture_file(root: &Path, relative_path: &str, content: &str) {
        let file_path = root.join(relative_path);
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(file_path, content).unwrap();
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
