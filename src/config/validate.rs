use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::bindings::{BindingValidationIssue, BindingValidationResult, validate_binding_registry};
use crate::diagnostics::{RunweaverDiagnostic, error_diagnostic};
use crate::surfaces::SurfaceTrigger;
use crate::toolchain::{
    ResolveManagedBinaryResult, managed_bin_dir, managed_toolchain_root, resolve_managed_binary,
};

use super::{
    CommandTask, RunweaverConfig, RunweaverDefinition, SeriesTask, TaskDefinition, ToolDefinition,
};

/// Result of [`validate_runweaver_definition`]: task/config diagnostics plus
/// binding validation, merged via `diagnostics()` and summarized by
/// `has_errors()`/`ok()`.
#[derive(Debug, Clone)]
pub struct RunweaverDefinitionValidation {
    pub config_diagnostics: Vec<RunweaverDiagnostic>,
    pub binding_validation: BindingValidationResult,
    pub binding_diagnostics: Vec<RunweaverDiagnostic>,
}

impl RunweaverDefinitionValidation {
    pub fn diagnostics(&self) -> Vec<RunweaverDiagnostic> {
        let mut diagnostics = self.config_diagnostics.clone();
        diagnostics.extend(self.binding_diagnostics.clone());
        diagnostics
    }

    pub fn has_errors(&self) -> bool {
        crate::diagnostics::has_error_diagnostics(&self.config_diagnostics)
            || crate::diagnostics::has_error_diagnostics(&self.binding_diagnostics)
            || !self.binding_validation.ok
    }

    pub fn ok(&self) -> bool {
        !self.has_errors()
    }
}

pub fn validate_config(config: &RunweaverConfig) -> Vec<RunweaverDiagnostic> {
    let mut diagnostics = Vec::new();
    let task_names = config.tasks.keys().cloned().collect::<HashSet<_>>();
    let policy_names = config.policies.keys().cloned().collect::<HashSet<_>>();

    for (tool_name, definition) in &config.tools {
        diagnostics.extend(validate_tool(tool_name, definition));
    }
    for (task_name, task) in &config.tasks {
        diagnostics.extend(validate_task(
            task_name,
            task,
            config,
            &task_names,
            &policy_names,
        ));
    }
    diagnostics.extend(detect_task_cycles(&config.tasks));
    diagnostics
}

/// Validates a full definition: dangling tool/policy/task references, task
/// cycles, and bindings that point at missing operations.
pub fn validate_runweaver_definition(
    definition: &RunweaverDefinition,
) -> RunweaverDefinitionValidation {
    let config_diagnostics = validate_config(&definition.task_config());
    let operation_names = definition
        .operations
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let binding_validation =
        validate_binding_registry(&definition.bindings, Some(&operation_names));
    let binding_diagnostics = binding_validation_diagnostics(&binding_validation.issues);

    RunweaverDefinitionValidation {
        config_diagnostics,
        binding_validation,
        binding_diagnostics,
    }
}

pub fn validate_project(
    config: &RunweaverConfig,
    cwd: impl AsRef<Path>,
) -> Vec<RunweaverDiagnostic> {
    let cwd = cwd.as_ref();
    let mut diagnostics = validate_config(config);
    let root = managed_toolchain_root(cwd);

    if !root.join("package.json").exists() {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_TOOLCHAIN_PACKAGE_MISSING",
                ".runweaver/package.json is missing.",
            )
            .with_path(".runweaver/package.json"),
        );
    }
    if !root.join("configs").exists() {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_CONFIGS_DIR_MISSING",
                ".runweaver/configs/ is missing.",
            )
            .with_path(".runweaver/configs"),
        );
    }
    if !root.join("bun.lock").exists() && !root.join("bun.lockb").exists() {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_LOCKFILE_MISSING",
                ".runweaver lockfile is missing; run `runweaver install`.",
            )
            .with_path(".runweaver/bun.lock"),
        );
    }
    if !managed_bin_dir(cwd).exists() {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_NODE_MODULES_MISSING",
                ".runweaver/node_modules/ is missing; run `runweaver install`.",
            )
            .with_path(".runweaver/node_modules"),
        );
    }
    if !gitignore_includes_runweaver_node_modules(cwd) {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_GITIGNORE_MISSING",
                ".gitignore must include .runweaver/node_modules/.",
            )
            .with_path(".gitignore"),
        );
    }

    for (tool_name, definition) in &config.tools {
        let ToolDefinition::Tool(definition) = definition else {
            continue;
        };
        if let Some(config) = &definition.config {
            let config_path = cwd.join(&config.path);
            if !config_path.exists() {
                diagnostics.push(
                    error_diagnostic(
                        "RUNWEAVER_TOOL_CONFIG_MISSING",
                        format!("Config for tool \"{tool_name}\" is missing."),
                    )
                    .with_path(config.path.clone()),
                );
            }
        }
        if let ResolveManagedBinaryResult::Missing { diagnostic } =
            resolve_managed_binary(cwd, &definition.program)
        {
            diagnostics.push(diagnostic);
        }
    }

    diagnostics
}

fn validate_tool(tool_name: &str, definition: &ToolDefinition) -> Vec<RunweaverDiagnostic> {
    let mut diagnostics = Vec::new();
    let (program, config) = match definition {
        ToolDefinition::Tool(definition) => (&definition.program, definition.config.as_ref()),
        ToolDefinition::HostCommand(definition) => (&definition.program, None),
    };

    if program.trim().is_empty() {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_INVALID_TOOL_PROGRAM",
                format!("Tool \"{tool_name}\" has an empty program."),
            )
            .with_path(format!("tools.{tool_name}")),
        );
    }
    if let Some(config) = config {
        if config.path.trim().is_empty() {
            diagnostics.push(
                error_diagnostic(
                    "RUNWEAVER_INVALID_TOOL_CONFIG",
                    format!("Tool \"{tool_name}\" has an empty config path."),
                )
                .with_path(format!("tools.{tool_name}.config.path")),
            );
        }
        if config.flag.trim().is_empty() {
            diagnostics.push(
                error_diagnostic(
                    "RUNWEAVER_INVALID_TOOL_CONFIG",
                    format!("Tool \"{tool_name}\" has an empty config flag."),
                )
                .with_path(format!("tools.{tool_name}.config.flag")),
            );
        }
    }

    diagnostics
}

fn validate_task(
    task_name: &str,
    task: &TaskDefinition,
    config: &RunweaverConfig,
    task_names: &HashSet<String>,
    policy_names: &HashSet<String>,
) -> Vec<RunweaverDiagnostic> {
    let mut diagnostics = Vec::new();
    for policy_ref in task.policies() {
        if !policy_names.contains(policy_ref) {
            diagnostics.push(
                error_diagnostic(
                    "RUNWEAVER_POLICY_REF_MISSING",
                    format!("Task \"{task_name}\" references missing policy \"{policy_ref}\"."),
                )
                .with_path(format!("tasks.{task_name}.policies")),
            );
        }
    }

    match task {
        TaskDefinition::Command(task) => {
            validate_command_task(task_name, task, config, &mut diagnostics)
        }
        TaskDefinition::Series(task) => {
            validate_composed_task(task_name, &task.refs, task_names, &mut diagnostics)
        }
        TaskDefinition::Parallel(task) => {
            validate_composed_task(task_name, &task.refs, task_names, &mut diagnostics)
        }
        TaskDefinition::Action(_) => {}
    }

    diagnostics
}

fn validate_command_task(
    task_name: &str,
    task: &CommandTask,
    config: &RunweaverConfig,
    diagnostics: &mut Vec<RunweaverDiagnostic>,
) {
    if !config.tools.contains_key(&task.tool) {
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_TOOL_REF_MISSING",
                format!(
                    "Task \"{task_name}\" references missing tool \"{}\".",
                    task.tool
                ),
            )
            .with_path(format!("tasks.{task_name}.tool")),
        );
    }
}

fn validate_composed_task(
    task_name: &str,
    refs: &[String],
    task_names: &HashSet<String>,
    diagnostics: &mut Vec<RunweaverDiagnostic>,
) {
    for (index, task_ref) in refs.iter().enumerate() {
        if !task_names.contains(task_ref) {
            diagnostics.push(
                error_diagnostic(
                    "RUNWEAVER_TASK_REF_MISSING",
                    format!("Task \"{task_name}\" references missing task \"{task_ref}\"."),
                )
                .with_path(format!("tasks.{task_name}.refs.{index}")),
            );
        }
    }
}

fn detect_task_cycles(tasks: &HashMap<String, TaskDefinition>) -> Vec<RunweaverDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut trail = Vec::new();

    for name in tasks.keys() {
        visit_task(
            name,
            tasks,
            &mut visiting,
            &mut visited,
            &mut trail,
            &mut diagnostics,
        );
    }

    diagnostics
}

fn visit_task(
    name: &str,
    tasks: &HashMap<String, TaskDefinition>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    trail: &mut Vec<String>,
    diagnostics: &mut Vec<RunweaverDiagnostic>,
) {
    if visiting.contains(name) {
        let mut cycle = trail.clone();
        cycle.push(name.to_owned());
        diagnostics.push(
            error_diagnostic(
                "RUNWEAVER_TASK_CYCLE",
                format!("Task cycle detected: {}.", cycle.join(" -> ")),
            )
            .with_path(format!("tasks.{name}")),
        );
        return;
    }
    if visited.contains(name) {
        return;
    }

    let Some(task) = tasks.get(name) else {
        return;
    };

    visiting.insert(name.to_owned());
    trail.push(name.to_owned());
    if let Some(refs) = task_refs(task) {
        for task_ref in refs {
            visit_task(task_ref, tasks, visiting, visited, trail, diagnostics);
        }
    }
    trail.pop();
    visiting.remove(name);
    visited.insert(name.to_owned());
}

fn task_refs(task: &TaskDefinition) -> Option<&[String]> {
    match task {
        TaskDefinition::Series(SeriesTask { refs, .. })
        | TaskDefinition::Parallel(super::ParallelTask { refs, .. }) => Some(refs),
        TaskDefinition::Command(_) | TaskDefinition::Action(_) => None,
    }
}

fn binding_validation_diagnostics(issues: &[BindingValidationIssue]) -> Vec<RunweaverDiagnostic> {
    issues
        .iter()
        .map(|issue| match issue {
            BindingValidationIssue::DuplicateTrigger {
                trigger,
                first_index,
                duplicate_index,
                operation_name,
                ..
            } => error_diagnostic(
                "RUNWEAVER_BINDING_TRIGGER_DUPLICATE",
                format!(
                    "Binding trigger {} is duplicated at indexes {first_index} and {duplicate_index} for operation \"{operation_name}\".",
                    format_trigger(trigger)
                ),
            )
            .with_path(format!("bindings.{duplicate_index}.trigger")),
            BindingValidationIssue::MissingOperation {
                trigger,
                binding_index,
                operation_name,
                ..
            } => error_diagnostic(
                "RUNWEAVER_BINDING_OPERATION_MISSING",
                format!(
                    "Binding trigger {} references missing operation \"{operation_name}\".",
                    format_trigger(trigger)
                ),
            )
            .with_path(format!("bindings.{binding_index}.operation")),
        })
        .collect()
}

pub(crate) fn format_binding_issues(issues: &[BindingValidationIssue]) -> String {
    issues
        .iter()
        .map(|issue| match issue {
            BindingValidationIssue::DuplicateTrigger {
                trigger,
                first_index,
                duplicate_index,
                operation_name,
                ..
            } => format!(
                "Duplicate binding trigger {} at indexes {first_index} and {duplicate_index} for operation \"{operation_name}\".",
                format_trigger(trigger)
            ),
            BindingValidationIssue::MissingOperation {
                trigger,
                binding_index,
                operation_name,
                ..
            } => format!(
                "Binding trigger {} at index {binding_index} references missing operation \"{operation_name}\".",
                format_trigger(trigger)
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_trigger(trigger: &SurfaceTrigger) -> String {
    match &trigger.phase {
        Some(phase) => format!("{}/{}:{phase}", trigger.surface, trigger.name),
        None => format!("{}/{}", trigger.surface, trigger.name),
    }
}

fn gitignore_includes_runweaver_node_modules(cwd: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(cwd.join(".gitignore")) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .any(|line| line == ".runweaver/node_modules/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::bindings::bind;
    use crate::config::{
        ActionResult, ActionTask, CommandArgs, CommandTask, PolicyDefinition, PolicyVerdict,
        ToolConfig, command, host_command, parallel, series, tool,
    };
    use crate::surfaces::SurfaceTrigger;

    fn allow(_: &crate::config::ExecutionContext) -> PolicyVerdict {
        PolicyVerdict::Allow
    }

    fn ok_action(_: &crate::config::ExecutionContext) -> ActionResult {
        ActionResult::success()
    }

    #[test]
    fn validate_config_reports_missing_refs_and_cycles() {
        let mut config = RunweaverConfig::new();
        config.tools.insert("ok".to_owned(), tool("ok", None));
        config
            .policies
            .insert("existing".to_owned(), PolicyDefinition::new(allow));
        config.tasks.insert(
            "missingTool".to_owned(),
            command("missing", CommandArgs::Static(Vec::new())),
        );
        config.tasks.insert(
            "missingPolicy".to_owned(),
            TaskDefinition::Command(CommandTask {
                tool: "ok".to_owned(),
                args: CommandArgs::Static(Vec::new()),
                result: None,
                policies: vec!["unknown".to_owned()],
            }),
        );
        config
            .tasks
            .insert("missingTask".to_owned(), series(&["nope"], false));
        config.tasks.insert("a".to_owned(), series(&["b"], false));
        config.tasks.insert("b".to_owned(), parallel(&["a"], false));

        let codes = validate_config(&config)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<HashSet<_>>();

        assert!(codes.contains("RUNWEAVER_TOOL_REF_MISSING"));
        assert!(codes.contains("RUNWEAVER_POLICY_REF_MISSING"));
        assert!(codes.contains("RUNWEAVER_TASK_REF_MISSING"));
        assert!(codes.contains("RUNWEAVER_TASK_CYCLE"));
    }

    #[test]
    fn validate_config_accepts_action_tasks_as_leaves() {
        let mut config = RunweaverConfig::new();
        config.tasks.insert(
            "prepare".to_owned(),
            TaskDefinition::Action(ActionTask::new(ok_action)),
        );

        assert_eq!(validate_config(&config), Vec::new());
    }

    #[test]
    fn validate_runweaver_definition_reports_config_and_binding_errors() {
        let mut definition = RunweaverDefinition::new();
        definition.tasks.insert(
            "missingTool".to_owned(),
            command("missing", CommandArgs::Static(Vec::new())),
        );
        definition.bindings.push(
            bind(SurfaceTrigger {
                surface: "cli".to_owned(),
                name: "count".to_owned(),
                phase: None,
            })
            .to("missingOperation")
            .finish(),
        );

        let validation = validate_runweaver_definition(&definition);
        let codes = validation
            .diagnostics()
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<HashSet<_>>();

        assert!(!validation.ok());
        assert!(!definition.validate().ok());
        assert!(!validation.binding_validation.ok);
        assert!(codes.contains("RUNWEAVER_TOOL_REF_MISSING"));
        assert!(codes.contains("RUNWEAVER_BINDING_OPERATION_MISSING"));
    }

    #[test]
    fn validate_config_reports_empty_tool_program_and_config_fields() {
        let mut config = RunweaverConfig::new();
        config.tools.insert(
            "bad".to_owned(),
            tool(
                " ",
                Some(ToolConfig {
                    path: " ".to_owned(),
                    flag: " ".to_owned(),
                }),
            ),
        );

        let codes = validate_config(&config)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();

        assert_eq!(
            codes,
            vec![
                "RUNWEAVER_INVALID_TOOL_PROGRAM",
                "RUNWEAVER_INVALID_TOOL_CONFIG",
                "RUNWEAVER_INVALID_TOOL_CONFIG"
            ]
        );
    }

    #[test]
    fn validate_project_does_not_require_host_commands_in_managed_toolchain() {
        let root = scaffold_root("host-command");
        let mut config = RunweaverConfig::new();
        config
            .tools
            .insert("runtime".to_owned(), host_command("missing-on-purpose"));
        config.tasks.insert(
            "runtime".to_owned(),
            command("runtime", CommandArgs::Static(Vec::new())),
        );

        let codes = validate_project(&config, &root)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();

        assert!(!codes.contains(&"RUNWEAVER_BINARY_MISSING".to_owned()));
    }

    #[test]
    fn validate_project_reports_missing_managed_tool_binary() {
        let root = scaffold_root("missing-bin");
        let mut config = RunweaverConfig::new();
        config
            .tools
            .insert("managed".to_owned(), tool("missing", None));

        let codes = validate_project(&config, &root)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"RUNWEAVER_BINARY_MISSING".to_owned()));
    }

    fn scaffold_root(label: &str) -> PathBuf {
        let root = test_root(label);
        std::fs::create_dir_all(root.join(".runweaver").join("configs")).unwrap();
        std::fs::create_dir_all(root.join(".runweaver").join("node_modules").join(".bin")).unwrap();
        std::fs::write(root.join(".runweaver").join("package.json"), "{}\n").unwrap();
        std::fs::write(root.join(".runweaver").join("bun.lock"), "\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".runweaver/node_modules/\n").unwrap();
        root
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "runweaver-validate-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
