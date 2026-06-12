use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::config::{RunweaverConfig, TaskCompletion, TaskRun, TaskRunStatus};
use crate::runtime::{CreateExecutionContextOptions, create_execution_context, run_task};

use super::contract::{HookEvent, HookStage};
use super::harness_hook_config::HookBinding;
use super::hook_command::{
    HookCommandSpec, HookFeedbackOutcome, ProjectedHookCommandOptions, define_hook_command,
    feedback_outcome, is_hook_feedback_outcome,
};

const MAX_HOOK_FAILURE_DETAIL_LENGTH: usize = 4_000;

pub type TaskHookRunner<Input> =
    Arc<dyn Fn(&str, Input) -> HookFeedbackOutcome + Send + Sync + 'static>;
pub type TaskHookCwdFn<Input> = Arc<dyn Fn(&Input) -> String + Send + Sync + 'static>;
pub type TaskHookFilesFn<Input> =
    Arc<dyn Fn(&Input) -> Option<Vec<String>> + Send + Sync + 'static>;
type TaskHookFilesCallback<Input> = dyn Fn(&Input) -> Option<Vec<String>> + Send + Sync + 'static;
pub type TaskHookFormatFailureFn = Arc<dyn Fn(&str, &TaskRun) -> String + Send + Sync + 'static>;

#[derive(Clone)]
pub struct TaskHookRunnerOptions<Input> {
    pub config: RunweaverConfig,
    pub cwd: Option<TaskHookCwdFn<Input>>,
    pub files: Option<TaskHookFilesFn<Input>>,
    pub format_failure: Option<TaskHookFormatFailureFn>,
}

impl<Input> TaskHookRunnerOptions<Input> {
    pub fn new(config: RunweaverConfig) -> Self {
        Self {
            config,
            cwd: None,
            files: None,
            format_failure: None,
        }
    }

    pub fn with_cwd(mut self, cwd: impl Fn(&Input) -> String + Send + Sync + 'static) -> Self {
        self.cwd = Some(Arc::new(cwd));
        self
    }

    pub fn with_files(
        mut self,
        files: impl Fn(&Input) -> Option<Vec<String>> + Send + Sync + 'static,
    ) -> Self {
        self.files = Some(Arc::new(files));
        self
    }

    pub fn with_format_failure(
        mut self,
        format_failure: impl Fn(&str, &TaskRun) -> String + Send + Sync + 'static,
    ) -> Self {
        self.format_failure = Some(Arc::new(format_failure));
        self
    }
}

impl<Input> std::fmt::Debug for TaskHookRunnerOptions<Input> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskHookRunnerOptions")
            .field("config", &self.config)
            .field("cwd", &self.cwd.as_ref().map(|_| "<fn>"))
            .field("files", &self.files.as_ref().map(|_| "<fn>"))
            .field(
                "format_failure",
                &self.format_failure.as_ref().map(|_| "<fn>"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub enum TaskHookRunnerSource<Input> {
    Runner(TaskHookRunner<Input>),
    Config(TaskHookRunnerOptions<Input>),
}

impl<Input> TaskHookRunnerSource<Input>
where
    Input: Serialize + Send + 'static,
{
    pub fn into_runner(self) -> TaskHookRunner<Input> {
        match self {
            Self::Runner(runner) => runner,
            Self::Config(options) => task_hook_runner(options),
        }
    }
}

pub struct DefineTaskHookOptions<Input, InputFn> {
    pub name: String,
    pub stage: HookStage,
    pub harnesses: Option<Vec<String>>,
    pub input: InputFn,
    pub task: String,
    pub runner: TaskHookRunnerSource<Input>,
    pub bindings: Vec<HookBinding>,
}

impl<Input, InputFn> DefineTaskHookOptions<Input, InputFn> {
    pub fn new(
        name: impl Into<String>,
        stage: HookStage,
        input: InputFn,
        task: impl Into<String>,
        runner: TaskHookRunnerSource<Input>,
        bindings: Vec<HookBinding>,
    ) -> Self {
        Self {
            name: name.into(),
            stage,
            harnesses: None,
            input,
            task: task.into(),
            runner,
            bindings,
        }
    }

    pub fn with_harnesses(
        mut self,
        harnesses: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.harnesses = Some(harnesses.into_iter().map(Into::into).collect());
        self
    }
}

/// Bridges a Runweaver task to the hook surface: the wrapping command spec
/// (which runs the task and converts its run into hook feedback) plus the
/// bindings that wire it into harness configs.
pub struct TaskHook {
    pub command: HookCommandSpec,
    pub bindings: Vec<HookBinding>,
}

impl std::fmt::Debug for TaskHook {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskHook")
            .field("command", &self.command)
            .field("bindings", &self.bindings)
            .finish()
    }
}

pub fn define_task_hook<Input, InputFn>(options: DefineTaskHookOptions<Input, InputFn>) -> TaskHook
where
    Input: Serialize + Send + 'static,
    InputFn: Fn(&HookEvent) -> anyhow::Result<Input> + Send + Sync + 'static,
{
    let runner = options.runner.into_runner();
    let task = options.task;
    let mut command_options = ProjectedHookCommandOptions::new(
        options.name,
        options.stage,
        options.input,
        move |input| Ok(runner(&task, input)),
        feedback_outcome,
    );
    if let Some(harnesses) = options.harnesses {
        command_options = command_options.with_harnesses(harnesses);
    }

    TaskHook {
        command: define_hook_command(command_options),
        bindings: options.bindings,
    }
}

pub fn task_hook_runner<Input>(options: TaskHookRunnerOptions<Input>) -> TaskHookRunner<Input>
where
    Input: Serialize + Send + 'static,
{
    let config = options.config;
    let cwd = options.cwd;
    let files = options.files;
    let format_failure = options
        .format_failure
        .unwrap_or_else(|| Arc::new(format_task_run_as_hook_block_reason));

    Arc::new(move |task_name, input| {
        let run = run_task_for_hook(&config, task_name, &input, cwd.as_deref(), files.as_deref());
        match run {
            Ok(run) => hook_feedback_from_task_run(task_name, &run, format_failure.as_ref()),
            Err(error) => HookFeedbackOutcome {
                block_reason: Some(format!("Runweaver hook task failed: {task_name}\n{error}")),
                system_message: None,
                updated_file: None,
            },
        }
    })
}

pub fn format_task_run_as_hook_block_reason(task_name: &str, run: &TaskRun) -> String {
    if run.status == TaskRunStatus::Denied {
        return join_non_empty([
            format!("Runweaver hook task failed: {task_name}"),
            run.reason.clone().unwrap_or_default(),
        ]);
    }

    if run.status == TaskRunStatus::Skipped {
        return join_non_empty([
            format!("Runweaver hook task skipped unexpectedly: {task_name}"),
            run.reason.clone().unwrap_or_default(),
        ]);
    }

    let mut details = vec![format!(
        "completion: {}",
        completion_label(run.completion.unwrap_or(TaskCompletion::Success))
    )];
    if let Some(output) = &run.output {
        append_output_detail(&mut details, "error", output.error.as_deref());
        append_output_detail(&mut details, "stderr", Some(&output.stderr));
        append_output_detail(&mut details, "stdout", Some(&output.stdout));
    }

    let mut lines = vec![format!("Runweaver hook task failed: {task_name}")];
    lines.extend(details);
    lines.join("\n")
}

pub fn hook_feedback_from_task_run(
    task_name: &str,
    run: &TaskRun,
    format_failure: &dyn Fn(&str, &TaskRun) -> String,
) -> HookFeedbackOutcome {
    if run.status == TaskRunStatus::Skipped {
        return HookFeedbackOutcome::default();
    }

    if run.status == TaskRunStatus::Denied
        || matches!(
            run.completion,
            Some(TaskCompletion::Error | TaskCompletion::ToolError)
        )
    {
        return HookFeedbackOutcome {
            block_reason: Some(format_failure(task_name, run)),
            system_message: None,
            updated_file: None,
        };
    }

    if let Some(data) = &run.data
        && is_hook_feedback_outcome(data)
        && let Ok(feedback) = serde_json::from_value::<HookFeedbackOutcome>(data.clone())
    {
        return feedback;
    }

    HookFeedbackOutcome {
        block_reason: Some(format_unexpected_task_hook_result(task_name, run)),
        system_message: None,
        updated_file: None,
    }
}

fn run_task_for_hook<Input>(
    config: &RunweaverConfig,
    task_name: &str,
    input: &Input,
    cwd: Option<&(dyn Fn(&Input) -> String + Send + Sync + 'static)>,
    files: Option<&TaskHookFilesCallback<Input>>,
) -> anyhow::Result<TaskRun>
where
    Input: Serialize,
{
    let input_value = serde_json::to_value(input)?;
    let cwd = cwd
        .map(|cwd| cwd(input))
        .or_else(|| cwd_from_input_value(&input_value))
        .map(Ok)
        .unwrap_or_else(current_dir_string)?;

    let mut context_options = CreateExecutionContextOptions::new(cwd);
    context_options.input = Some(input_value);
    if let Some(files) = files.and_then(|files| files(input)) {
        context_options.files = files;
    }

    run_task(config, task_name, create_execution_context(context_options))
}

fn cwd_from_input_value(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("cwd"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn current_dir_string() -> anyhow::Result<String> {
    Ok(path_to_string(std::env::current_dir()?))
}

fn path_to_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

fn format_unexpected_task_hook_result(task_name: &str, run: &TaskRun) -> String {
    let completion = if run.status == TaskRunStatus::Completed {
        format!(
            "\ncompletion: {}",
            completion_label(run.completion.unwrap_or(TaskCompletion::Success))
        )
    } else {
        String::new()
    };
    format!(
        "Runweaver hook task returned invalid feedback outcome: {task_name}\nExpected task data to match HookFeedbackOutcome.{completion}"
    )
}

fn append_output_detail(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    if value.is_empty() {
        return;
    }
    lines.push(format!(
        "{label}: {}",
        truncate_hook_failure_detail(value.trim_end())
    ));
}

fn truncate_hook_failure_detail(value: &str) -> String {
    if value.chars().count() <= MAX_HOOK_FAILURE_DETAIL_LENGTH {
        return value.to_owned();
    }
    let truncated = value
        .chars()
        .take(MAX_HOOK_FAILURE_DETAIL_LENGTH)
        .collect::<String>();
    format!("{truncated}…")
}

fn completion_label(completion: TaskCompletion) -> &'static str {
    match completion {
        TaskCompletion::Success => "success",
        TaskCompletion::Warning => "warning",
        TaskCompletion::Error => "error",
        TaskCompletion::ToolError => "toolError",
    }
}

fn join_non_empty(lines: impl IntoIterator<Item = String>) -> String {
    lines
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::Serialize;
    use serde_json::json;

    use super::super::contract::HookOutcome;
    use crate::config::{
        ActionResult, TaskDefinition, TaskKind, TaskOutput, action, define_config,
    };

    use super::*;

    #[derive(Debug, Clone, Serialize)]
    struct FixtureInput {
        cwd: String,
        tag: Option<String>,
    }

    fn feedback_action(ctx: &crate::config::ExecutionContext) -> ActionResult {
        let tag = ctx
            .input
            .as_ref()
            .and_then(|input| input.get("tag"))
            .and_then(Value::as_str)
            .unwrap_or("missing");
        ActionResult::Completed {
            completion: TaskCompletion::Success,
            output: TaskOutput::success(),
            data: Some(json!({ "systemMessage": format!("{}:{tag}", ctx.cwd) })),
            next_context: None,
        }
    }

    fn skipped_action(_ctx: &crate::config::ExecutionContext) -> ActionResult {
        ActionResult::Skipped {
            reason: Some("not relevant".to_owned()),
        }
    }

    fn denied_action(_ctx: &crate::config::ExecutionContext) -> ActionResult {
        ActionResult::Denied {
            reason: "blocked by task".to_owned(),
        }
    }

    fn errored_action(_ctx: &crate::config::ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Error,
            output: TaskOutput {
                exit_code: Some(1),
                stdout: String::new(),
                stderr: "lint failed\n".to_owned(),
                error: None,
            },
            data: Some(json!({ "systemMessage": "ignored on blocking completion" })),
            next_context: None,
        }
    }

    fn invalid_action(_ctx: &crate::config::ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Success,
            output: TaskOutput::success(),
            data: Some(json!({ "blockReason": 1 })),
            next_context: None,
        }
    }

    fn feedback_config() -> RunweaverConfig {
        define_config(RunweaverConfig {
            tools: HashMap::new(),
            policies: HashMap::new(),
            tasks: HashMap::from([(
                "feedback".to_owned(),
                TaskDefinition::Action(crate::config::ActionTask::new(feedback_action)),
            )]),
        })
    }

    fn state_config() -> RunweaverConfig {
        define_config(RunweaverConfig {
            tools: HashMap::new(),
            policies: HashMap::new(),
            tasks: HashMap::from([
                ("skipped".to_owned(), action(skipped_action)),
                ("denied".to_owned(), action(denied_action)),
                ("errored".to_owned(), action(errored_action)),
                ("invalid".to_owned(), action(invalid_action)),
            ]),
        })
    }

    fn fixture_event(overrides: Option<(&str, &str)>) -> HookEvent {
        HookEvent {
            harness: "fixture".to_owned(),
            stage: HookStage::PreTool,
            session_id: overrides
                .filter(|(key, _)| *key == "session_id")
                .map(|(_, value)| value.to_owned())
                .unwrap_or_else(|| "session-1".to_owned()),
            tool_call_id: None,
            transcript_path: None,
            cwd: overrides
                .filter(|(key, _)| *key == "cwd")
                .map(|(_, value)| value.to_owned())
                .unwrap_or_else(|| "/repo".to_owned()),
            touched_path_candidates: Vec::new(),
            patch_text: None,
            tool_command: None,
            tool_name: None,
            tool_response: None,
            stop_hook_active: false,
        }
    }

    #[test]
    fn task_hook_runner_runs_task_with_projected_input_and_returns_feedback_data() {
        let runner = task_hook_runner(TaskHookRunnerOptions::new(feedback_config()));
        let input = FixtureInput {
            cwd: "/repo".to_owned(),
            tag: Some("ready".to_owned()),
        };

        let result = runner("feedback", input);

        assert_eq!(
            result,
            HookFeedbackOutcome {
                block_reason: None,
                system_message: Some("/repo:ready".to_owned()),
                updated_file: None,
            }
        );
    }

    #[test]
    fn maps_skipped_denied_blocking_and_invalid_task_runs_into_hook_feedback() {
        let runner = task_hook_runner(TaskHookRunnerOptions::new(state_config()));
        let input = FixtureInput {
            cwd: "/repo".to_owned(),
            tag: None,
        };

        let skipped = runner("skipped", input.clone());
        let denied = runner("denied", input.clone());
        let errored = runner("errored", input.clone());
        let invalid = runner("invalid", input);

        assert_eq!(skipped, HookFeedbackOutcome::default());
        assert_eq!(
            denied.block_reason.as_deref(),
            Some("Runweaver hook task failed: denied\nblocked by task")
        );
        assert_eq!(
            errored.block_reason.as_deref(),
            Some("Runweaver hook task failed: errored\ncompletion: error\nstderr: lint failed")
        );
        assert_eq!(
            invalid.block_reason.as_deref(),
            Some(
                "Runweaver hook task returned invalid feedback outcome: invalid\nExpected task data to match HookFeedbackOutcome.\ncompletion: success"
            )
        );
    }

    #[test]
    fn define_task_hook_builds_hook_command_and_bindings_around_runner() {
        let runner = task_hook_runner(TaskHookRunnerOptions::new(feedback_config()));
        let hook = define_task_hook(DefineTaskHookOptions::new(
            "guard-example",
            HookStage::PreTool,
            |event: &HookEvent| {
                Ok(FixtureInput {
                    cwd: event.cwd.clone(),
                    tag: Some(event.session_id.clone()),
                })
            },
            "feedback",
            TaskHookRunnerSource::Runner(runner),
            vec![HookBinding::new("fixture", 10, "Check Fixture")],
        ));

        let outcome = hook
            .command
            .run(&fixture_event(Some(("session_id", "abc"))))
            .unwrap();

        assert_eq!(
            outcome,
            HookOutcome::Pass {
                system_message: Some("/repo:abc".to_owned()),
                updated_file: None,
            }
        );
        assert_eq!(hook.command.name(), "guard-example");
        assert_eq!(
            hook.bindings,
            vec![HookBinding::new("fixture", 10, "Check Fixture")]
        );
    }

    #[test]
    fn define_task_hook_can_build_runner_from_config_options() {
        let hook = define_task_hook(DefineTaskHookOptions::new(
            "guard-config",
            HookStage::PreTool,
            |event: &HookEvent| {
                Ok(FixtureInput {
                    cwd: event.cwd.clone(),
                    tag: Some("ready".to_owned()),
                })
            },
            "feedback",
            TaskHookRunnerSource::Config(TaskHookRunnerOptions::new(feedback_config())),
            Vec::new(),
        ));

        let outcome = hook.command.run(&fixture_event(None)).unwrap();

        assert_eq!(
            outcome,
            HookOutcome::Pass {
                system_message: Some("/repo:ready".to_owned()),
                updated_file: None,
            }
        );
    }

    #[test]
    fn format_task_run_as_hook_block_reason_truncates_long_output_details() {
        let run = TaskRun {
            task_name: "long".to_owned(),
            task_type: TaskKind::Action,
            status: TaskRunStatus::Completed,
            completion: Some(TaskCompletion::Error),
            output: Some(TaskOutput {
                exit_code: Some(1),
                stdout: "x".repeat(MAX_HOOK_FAILURE_DETAIL_LENGTH + 1),
                stderr: String::new(),
                error: None,
            }),
            data: None,
            next_context: None,
            children: Vec::new(),
            reason: None,
        };

        let reason = format_task_run_as_hook_block_reason("long", &run);

        assert!(reason.ends_with('…'));
    }
}
