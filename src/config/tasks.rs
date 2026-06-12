use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The scoped environment a task or policy runs in: working directory, env
/// vars, in-scope files, optional JSON input, and the runs that came before
/// it in a series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionContext {
    pub cwd: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(default)]
    pub previous_runs: Vec<TaskRun>,
}

impl ExecutionContext {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            env: HashMap::new(),
            files: Vec::new(),
            consumer: None,
            mode: None,
            input: None,
            previous_runs: Vec::new(),
        }
    }

    pub fn with_input(mut self, input: Value) -> Self {
        self.input = Some(input);
        self
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files = files;
        self
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }
}

/// Partial context update an action returns to mutate the
/// [`ExecutionContext`] of subsequent steps in a series; every field is
/// optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextExecutionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

impl NextExecutionContext {
    pub fn new() -> Self {
        Self {
            cwd: None,
            env: None,
            files: None,
            consumer: None,
            mode: None,
            input: None,
        }
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    pub fn with_files(mut self, files: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.files = Some(files.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_consumer(mut self, consumer: impl Into<String>) -> Self {
        self.consumer = Some(consumer.into());
        self
    }

    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    pub fn with_input(mut self, input: Value) -> Self {
        self.input = Some(input);
        self
    }
}

impl Default for NextExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

/// What an action closure reports back: completed (with completion, output,
/// optional data and context update), skipped, or denied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ActionResult {
    #[serde(rename = "completed")]
    Completed {
        #[serde(default = "success_completion")]
        completion: TaskCompletion,
        #[serde(default)]
        output: TaskOutput,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
        #[serde(
            default,
            rename = "nextContext",
            skip_serializing_if = "Option::is_none"
        )]
        next_context: Option<Box<NextExecutionContext>>,
    },
    #[serde(rename = "skipped")]
    Skipped {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "denied")]
    Denied { reason: String },
}

impl ActionResult {
    pub fn success() -> Self {
        Self::completed().build()
    }

    pub fn completed() -> CompletedActionResultBuilder {
        CompletedActionResultBuilder::default()
    }

    pub fn skipped() -> Self {
        Self::Skipped { reason: None }
    }

    pub fn skipped_with_reason(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: Some(reason.into()),
        }
    }

    pub fn denied(reason: impl Into<String>) -> Self {
        Self::Denied {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedActionResultBuilder {
    completion: TaskCompletion,
    output: TaskOutput,
    data: Option<Value>,
    next_context: Option<NextExecutionContext>,
}

impl CompletedActionResultBuilder {
    pub fn completion(mut self, completion: TaskCompletion) -> Self {
        self.completion = completion;
        self
    }

    pub fn output(mut self, output: TaskOutput) -> Self {
        self.output = output;
        self
    }

    pub fn data(mut self, data: impl Into<Value>) -> Self {
        self.data = Some(data.into());
        self
    }

    pub fn next_context(mut self, next_context: NextExecutionContext) -> Self {
        self.next_context = Some(next_context);
        self
    }

    pub fn build(self) -> ActionResult {
        ActionResult::Completed {
            completion: self.completion,
            output: self.output,
            data: self.data,
            next_context: self.next_context.map(Box::new),
        }
    }
}

impl Default for CompletedActionResultBuilder {
    fn default() -> Self {
        Self {
            completion: TaskCompletion::Success,
            output: TaskOutput::success(),
            data: None,
            next_context: None,
        }
    }
}

impl From<CompletedActionResultBuilder> for ActionResult {
    fn from(builder: CompletedActionResultBuilder) -> Self {
        builder.build()
    }
}

/// How a completed task ended. `Error` is a failed check; `ToolError` means
/// the tool itself misbehaved (could not run, unmapped exit code). Both are
/// blocking; `Warning` and `Success` are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskCompletion {
    Success,
    Warning,
    Error,
    ToolError,
}

fn success_completion() -> TaskCompletion {
    TaskCompletion::Success
}

/// Captured process output of a task. `error` reports tool-level failures
/// (spawn errors, missing binaries), distinct from a non-zero `exit_code`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Default for TaskOutput {
    fn default() -> Self {
        Self::success()
    }
}

impl TaskOutput {
    pub fn new(
        exit_code: Option<i32>,
        stdout: impl Into<String>,
        stderr: impl Into<String>,
    ) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
            error: None,
        }
    }

    pub fn success() -> Self {
        Self {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        }
    }

    pub fn error(exit_code: i32, stderr: impl Into<String>) -> Self {
        Self::new(Some(exit_code), String::new(), stderr)
    }

    pub fn tool_error(error: impl Into<String>) -> Self {
        Self {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.into()),
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
}

/// The full record of one task execution: status, completion, output, and
/// child runs for series/parallel composites. Skipped and denied runs carry
/// their `reason` instead of a completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub task_name: String,
    pub task_type: TaskKind,
    pub status: TaskRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<TaskCompletion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<TaskOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_context: Option<NextExecutionContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TaskRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskRunStatus {
    Completed,
    Skipped,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskKind {
    Command,
    Action,
    Series,
    Parallel,
}

/// The task-runner view of a definition: just tools, tasks, and policies.
/// Derived from [`RunweaverDefinition::task_config`](super::RunweaverDefinition::task_config)
/// or authored directly with [`define_config_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunweaverConfig {
    pub tools: HashMap<String, ToolDefinition>,
    pub tasks: HashMap<String, TaskDefinition>,
    pub policies: HashMap<String, PolicyDefinition>,
}

impl RunweaverConfig {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            tasks: HashMap::new(),
            policies: HashMap::new(),
        }
    }
}

impl Default for RunweaverConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunweaverConfigBuilder {
    config: RunweaverConfig,
}

impl RunweaverConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tool(&mut self, name: impl Into<String>, definition: ToolDefinition) -> &mut Self {
        self.config.tools.insert(name.into(), definition);
        self
    }

    pub fn policy(&mut self, name: impl Into<String>, definition: PolicyDefinition) -> &mut Self {
        self.config.policies.insert(name.into(), definition);
        self
    }

    pub fn task(
        &mut self,
        name: impl Into<String>,
        definition: impl Into<TaskDefinition>,
    ) -> &mut Self {
        self.config.tasks.insert(name.into(), definition.into());
        self
    }

    pub fn build(self) -> RunweaverConfig {
        self.config
    }
}

pub fn define_config(config: RunweaverConfig) -> RunweaverConfig {
    config
}

/// Builds a [`RunweaverConfig`] through a builder closure.
pub fn define_config_with(configure: impl FnOnce(&mut RunweaverConfigBuilder)) -> RunweaverConfig {
    let mut builder = RunweaverConfigBuilder::new();
    configure(&mut builder);
    define_config(builder.build())
}

/// An invocable tool: either managed by the Runweaver toolchain (resolved
/// from `.runweaver/`) or a command expected on the host `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolDefinition {
    Tool(ManagedToolDefinition),
    HostCommand(HostCommandDefinition),
}

impl ToolDefinition {
    pub fn managed(program: impl Into<String>) -> Self {
        Self::Tool(ManagedToolDefinition::new(program))
    }

    pub fn managed_with_config(program: impl Into<String>, config: ToolConfig) -> Self {
        Self::Tool(ManagedToolDefinition::new(program).with_config(config))
    }

    pub fn host_command(program: impl Into<String>) -> Self {
        Self::HostCommand(HostCommandDefinition::new(program))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedToolDefinition {
    pub program: String,
    pub config: Option<ToolConfig>,
}

impl ManagedToolDefinition {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            config: None,
        }
    }

    pub fn with_config(mut self, config: ToolConfig) -> Self {
        self.config = Some(config);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCommandDefinition {
    pub program: String,
}

impl HostCommandDefinition {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolConfig {
    pub path: String,
    pub flag: String,
}

impl ToolConfig {
    pub fn new(path: impl Into<String>, flag: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            flag: flag.into(),
        }
    }
}

#[derive(Clone)]
pub struct PolicyDefinition {
    pub evaluate: PolicyFn,
}

impl PolicyDefinition {
    pub fn new(
        evaluate: impl Fn(&ExecutionContext) -> PolicyVerdict + Send + Sync + 'static,
    ) -> Self {
        Self {
            evaluate: Arc::new(evaluate),
        }
    }
}

impl std::fmt::Debug for PolicyDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PolicyDefinition")
            .finish_non_exhaustive()
    }
}

impl PartialEq for PolicyDefinition {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.evaluate, &other.evaluate)
    }
}

impl Eq for PolicyDefinition {}

pub type PolicyFn = Arc<dyn Fn(&ExecutionContext) -> PolicyVerdict + Send + Sync + 'static>;
pub type ActionFn = Arc<dyn Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static>;
pub type CommandArgsFn = Arc<dyn Fn(&ExecutionContext) -> Vec<String> + Send + Sync + 'static>;

/// A policy's decision about a task: run it, skip it (non-blocking, with an
/// optional reason), or deny it (blocking, with a reason).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    Allow,
    Skip { reason: Option<String> },
    Deny { reason: String },
}

impl PolicyVerdict {
    pub fn allow() -> Self {
        Self::Allow
    }

    pub fn skip() -> Self {
        Self::Skip { reason: None }
    }

    pub fn skip_with_reason(reason: impl Into<String>) -> Self {
        Self::Skip {
            reason: Some(reason.into()),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }
}

pub fn policy(
    evaluate: impl Fn(&ExecutionContext) -> PolicyVerdict + Send + Sync + 'static,
) -> PolicyDefinition {
    PolicyDefinition::new(evaluate)
}

pub fn allow() -> PolicyVerdict {
    PolicyVerdict::allow()
}

pub fn skip(reason: Option<&str>) -> PolicyVerdict {
    match reason {
        Some(reason) => skip_with_reason(reason),
        None => PolicyVerdict::skip(),
    }
}

pub fn skip_with_reason(reason: impl Into<String>) -> PolicyVerdict {
    PolicyVerdict::skip_with_reason(reason)
}

pub fn deny(reason: impl Into<String>) -> PolicyVerdict {
    PolicyVerdict::deny(reason)
}

/// Command-task arguments: a fixed list, or a closure computing them from
/// the [`ExecutionContext`] at run time.
#[derive(Clone)]
pub enum CommandArgs {
    Static(Vec<String>),
    Dynamic(CommandArgsFn),
}

impl CommandArgs {
    pub fn static_args(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Static(args.into_iter().map(Into::into).collect())
    }

    pub fn dynamic(
        args: impl Fn(&ExecutionContext) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        Self::Dynamic(Arc::new(args))
    }
}

pub type TaskPolicies = Vec<String>;

impl std::fmt::Debug for CommandArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static(args) => formatter.debug_tuple("Static").field(args).finish(),
            Self::Dynamic(_) => formatter.debug_tuple("Dynamic").field(&"<fn>").finish(),
        }
    }
}

impl PartialEq for CommandArgs {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static(left), Self::Static(right)) => left == right,
            (Self::Dynamic(left), Self::Dynamic(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl Eq for CommandArgs {}

/// A unit of project work: invoke a tool (`Command`), run a Rust closure
/// (`Action`), or compose other tasks by name (`Series`, `Parallel`).
#[derive(Clone)]
pub enum TaskDefinition {
    Command(CommandTask),
    Action(ActionTask),
    Series(SeriesTask),
    Parallel(ParallelTask),
}

#[derive(Clone)]
pub struct CommandTaskOptions {
    pub args: CommandArgs,
    pub result: Option<ResultMapping>,
    pub policies: Vec<String>,
}

impl CommandTaskOptions {
    pub fn new(args: CommandArgs) -> Self {
        Self {
            args,
            result: None,
            policies: Vec::new(),
        }
    }

    pub fn with_result(mut self, result: ResultMapping) -> Self {
        self.result = Some(result);
        self
    }

    pub fn with_policies(mut self, policies: &[&str]) -> Self {
        self.policies = policies.iter().map(|policy| (*policy).to_owned()).collect();
        self
    }
}

impl std::fmt::Debug for CommandTaskOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandTaskOptions")
            .field("args", &self.args)
            .field("result", &self.result)
            .field("policies", &self.policies)
            .finish()
    }
}

impl PartialEq for CommandTaskOptions {
    fn eq(&self, other: &Self) -> bool {
        self.args == other.args && self.result == other.result && self.policies == other.policies
    }
}

impl Eq for CommandTaskOptions {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionTaskOptions {
    pub policies: Vec<String>,
}

impl ActionTaskOptions {
    pub fn with_policies(mut self, policies: &[&str]) -> Self {
        self.policies = policies.iter().map(|policy| (*policy).to_owned()).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompositeTaskOptions {
    pub fail_fast: bool,
    pub policies: Vec<String>,
}

impl CompositeTaskOptions {
    pub fn fail_fast() -> Self {
        Self {
            fail_fast: true,
            policies: Vec::new(),
        }
    }

    pub fn with_policies(mut self, policies: &[&str]) -> Self {
        self.policies = policies.iter().map(|policy| (*policy).to_owned()).collect();
        self
    }
}

impl std::fmt::Debug for TaskDefinition {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(task) => formatter.debug_tuple("Command").field(task).finish(),
            Self::Action(task) => formatter.debug_tuple("Action").field(task).finish(),
            Self::Series(task) => formatter.debug_tuple("Series").field(task).finish(),
            Self::Parallel(task) => formatter.debug_tuple("Parallel").field(task).finish(),
        }
    }
}

impl PartialEq for TaskDefinition {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Command(left), Self::Command(right)) => left == right,
            (Self::Action(left), Self::Action(right)) => left == right,
            (Self::Series(left), Self::Series(right)) => left == right,
            (Self::Parallel(left), Self::Parallel(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for TaskDefinition {}

impl TaskDefinition {
    pub fn kind(&self) -> TaskKind {
        match self {
            Self::Command(_) => TaskKind::Command,
            Self::Action(_) => TaskKind::Action,
            Self::Series(_) => TaskKind::Series,
            Self::Parallel(_) => TaskKind::Parallel,
        }
    }

    pub fn policies(&self) -> &[String] {
        match self {
            Self::Command(task) => &task.policies,
            Self::Action(task) => &task.policies,
            Self::Series(task) => &task.policies,
            Self::Parallel(task) => &task.policies,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTask {
    pub tool: String,
    pub args: CommandArgs,
    pub result: Option<ResultMapping>,
    pub policies: Vec<String>,
}

#[derive(Clone)]
pub struct ActionTask {
    pub run: ActionFn,
    pub policies: Vec<String>,
}

impl ActionTask {
    pub fn new(run: impl Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static) -> Self {
        Self {
            run: Arc::new(run),
            policies: Vec::new(),
        }
    }

    pub fn with_policies(mut self, policies: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.policies = policies.into_iter().map(Into::into).collect();
        self
    }
}

impl std::fmt::Debug for ActionTask {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActionTask")
            .field("run", &"<fn>")
            .field("policies", &self.policies)
            .finish()
    }
}

impl PartialEq for ActionTask {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.run, &other.run) && self.policies == other.policies
    }
}

impl Eq for ActionTask {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesTask {
    pub refs: Vec<String>,
    pub fail_fast: bool,
    pub policies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelTask {
    pub refs: Vec<String>,
    pub fail_fast: bool,
    pub policies: Vec<String>,
}

/// Maps a command's exit codes to a [`TaskCompletion`]; codes matching no
/// rule fall through to the `error`/`tool_error` [`ExitCodeRule`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultMapping {
    pub success: Option<Vec<i32>>,
    pub warning: Option<Vec<i32>>,
    pub error: ExitCodeRule,
    pub tool_error: ExitCodeRule,
}

impl Default for ResultMapping {
    fn default() -> Self {
        Self {
            success: None,
            warning: None,
            error: ExitCodeRule::Unset,
            tool_error: ExitCodeRule::Unset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitCodeRule {
    Codes(Vec<i32>),
    Otherwise,
    Unset,
}

pub fn is_blocking_completion(completion: TaskCompletion) -> bool {
    matches!(
        completion,
        TaskCompletion::Error | TaskCompletion::ToolError
    )
}

pub fn tool(program: impl Into<String>, config: Option<ToolConfig>) -> ToolDefinition {
    match config {
        Some(config) => ToolDefinition::managed_with_config(program, config),
        None => ToolDefinition::managed(program),
    }
}

pub fn host_command(program: impl Into<String>) -> ToolDefinition {
    ToolDefinition::host_command(program)
}

pub fn command(tool: impl Into<String>, args: CommandArgs) -> TaskDefinition {
    command_with_options(tool, CommandTaskOptions::new(args))
}

pub fn command_with_options(
    tool: impl Into<String>,
    options: CommandTaskOptions,
) -> TaskDefinition {
    TaskDefinition::Command(CommandTask {
        tool: tool.into(),
        args: options.args,
        result: options.result,
        policies: options.policies,
    })
}

pub fn action(
    run: impl Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static,
) -> TaskDefinition {
    action_with_options(run, ActionTaskOptions::default())
}

pub fn action_with_options(
    run: impl Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static,
    options: ActionTaskOptions,
) -> TaskDefinition {
    TaskDefinition::Action(ActionTask::new(run).with_policies(options.policies))
}

pub fn series(refs: &[&str], fail_fast: bool) -> TaskDefinition {
    series_with_options(
        refs,
        CompositeTaskOptions {
            fail_fast,
            policies: Vec::new(),
        },
    )
}

pub fn series_with_options(refs: &[&str], options: CompositeTaskOptions) -> TaskDefinition {
    TaskDefinition::Series(SeriesTask {
        refs: refs.iter().map(|task| (*task).to_owned()).collect(),
        fail_fast: options.fail_fast,
        policies: options.policies,
    })
}

pub fn parallel(refs: &[&str], fail_fast: bool) -> TaskDefinition {
    parallel_with_options(
        refs,
        CompositeTaskOptions {
            fail_fast,
            policies: Vec::new(),
        },
    )
}

pub fn parallel_with_options(refs: &[&str], options: CompositeTaskOptions) -> TaskDefinition {
    TaskDefinition::Parallel(ParallelTask {
        refs: refs.iter().map(|task| (*task).to_owned()).collect(),
        fail_fast: options.fail_fast,
        policies: options.policies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_policy(_: &ExecutionContext) -> PolicyVerdict {
        allow()
    }

    fn ok_action(_: &ExecutionContext) -> ActionResult {
        ActionResult::success()
    }

    #[test]
    fn policy_helpers_create_gate_verdicts() {
        assert_eq!(allow(), PolicyVerdict::Allow);
        assert_eq!(skip(None), PolicyVerdict::skip());
        assert_eq!(
            skip(Some("docs")),
            PolicyVerdict::Skip {
                reason: Some("docs".to_owned())
            }
        );
        assert_eq!(
            skip_with_reason("docs"),
            PolicyVerdict::skip_with_reason("docs")
        );
        assert_eq!(deny("blocked"), PolicyVerdict::deny("blocked"));
    }

    #[test]
    fn tool_helpers_create_configured_managed_and_host_definitions() {
        let config = ToolConfig::new(".runweaver/configs/tool.json", "--config");
        assert_eq!(
            ManagedToolDefinition::new("oxlint").with_config(config.clone()),
            ManagedToolDefinition {
                program: "oxlint".to_owned(),
                config: Some(config.clone())
            }
        );
        assert_eq!(
            HostCommandDefinition::new("cargo"),
            HostCommandDefinition {
                program: "cargo".to_owned()
            }
        );
        assert_eq!(
            ToolDefinition::managed_with_config("oxlint", config.clone()),
            tool("oxlint", Some(config))
        );
        assert_eq!(ToolDefinition::host_command("cargo"), host_command("cargo"));
    }

    #[test]
    fn command_with_options_preserves_policies_and_result_mapping() {
        let result = ResultMapping {
            success: Some(vec![0]),
            warning: Some(vec![2]),
            error: ExitCodeRule::Otherwise,
            tool_error: ExitCodeRule::Unset,
        };
        let task = command_with_options(
            "lint",
            CommandTaskOptions::new(CommandArgs::Static(vec!["--quiet".to_owned()]))
                .with_policies(&["source"])
                .with_result(result.clone()),
        );

        let TaskDefinition::Command(task) = task else {
            panic!("expected command task");
        };
        assert_eq!(task.policies, vec!["source"]);
        assert_eq!(task.result, Some(result));
    }

    #[test]
    fn action_result_builders_preserve_completed_skipped_denied_shapes() {
        let completed = ActionResult::completed()
            .completion(TaskCompletion::Warning)
            .output(TaskOutput::new(Some(2), "warn\n", ""))
            .data(serde_json::json!({ "changed": true }))
            .next_context(NextExecutionContext::new().with_files(["src/lib.rs"]))
            .build();

        assert_eq!(
            completed,
            ActionResult::Completed {
                completion: TaskCompletion::Warning,
                output: TaskOutput::new(Some(2), "warn\n", ""),
                data: Some(serde_json::json!({ "changed": true })),
                next_context: Some(Box::new(
                    NextExecutionContext::new().with_files(["src/lib.rs"])
                )),
            }
        );
        assert_eq!(
            ActionResult::skipped(),
            ActionResult::Skipped { reason: None }
        );
        assert_eq!(
            ActionResult::skipped_with_reason("docs"),
            ActionResult::Skipped {
                reason: Some("docs".to_owned())
            }
        );
        assert_eq!(
            ActionResult::denied("blocked"),
            ActionResult::Denied {
                reason: "blocked".to_owned()
            }
        );
    }

    #[test]
    fn task_output_helpers_preserve_process_and_tool_error_fields() {
        assert_eq!(
            TaskOutput::error(1, "failed\n"),
            TaskOutput {
                exit_code: Some(1),
                stdout: String::new(),
                stderr: "failed\n".to_owned(),
                error: None,
            }
        );
        assert_eq!(
            TaskOutput::tool_error("missing binary"),
            TaskOutput {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some("missing binary".to_owned()),
            }
        );
    }

    #[test]
    fn action_and_composite_options_preserve_policies() {
        let action = action_with_options(
            ok_action,
            ActionTaskOptions::default().with_policies(&["source"]),
        );
        let series = series_with_options(
            &["a", "b"],
            CompositeTaskOptions::fail_fast().with_policies(&["source"]),
        );

        assert_eq!(action.policies(), &["source".to_owned()]);
        assert_eq!(series.policies(), &["source".to_owned()]);
    }

    #[test]
    fn define_config_preserves_config_maps() {
        let mut config = RunweaverConfig::new();
        config.tools.insert("bun".to_owned(), host_command("bun"));
        config
            .policies
            .insert("ok".to_owned(), policy(allow_policy));

        let defined = define_config(config);

        assert!(defined.tools.contains_key("bun"));
        assert!(defined.policies.contains_key("ok"));
    }

    #[test]
    fn define_config_with_builds_tools_policies_and_tasks() {
        let config = define_config_with(|config| {
            config
                .tool("cargo", host_command("cargo"))
                .policy("ok", policy(allow_policy))
                .task("prepare", action(ok_action));
        });

        assert!(config.tools.contains_key("cargo"));
        assert!(config.policies.contains_key("ok"));
        assert!(config.tasks.contains_key("prepare"));
    }

    #[test]
    fn task_run_serializes_with_public_camel_case_fields() {
        let run = TaskRun {
            task_name: "check".to_owned(),
            task_type: TaskKind::Series,
            status: TaskRunStatus::Completed,
            completion: Some(TaskCompletion::Error),
            output: Some(TaskOutput {
                exit_code: Some(1),
                stdout: "out\n".to_owned(),
                stderr: "err\n".to_owned(),
                error: Some("failed".to_owned()),
            }),
            data: Some(serde_json::json!({ "ready": true })),
            next_context: Some(NextExecutionContext {
                cwd: None,
                env: None,
                files: Some(vec!["src/lib.rs".to_owned()]),
                consumer: None,
                mode: None,
                input: None,
            }),
            children: Vec::new(),
            reason: None,
        };

        assert_eq!(
            serde_json::to_value(&run).unwrap(),
            serde_json::json!({
                "taskName": "check",
                "taskType": "series",
                "status": "completed",
                "completion": "error",
                "output": {
                    "exitCode": 1,
                    "stdout": "out\n",
                    "stderr": "err\n",
                    "error": "failed"
                },
                "data": { "ready": true },
                "nextContext": {
                    "files": ["src/lib.rs"]
                }
            })
        );
    }

    #[test]
    fn execution_context_serializes_previous_runs_as_public_camel_case() {
        let ctx = ExecutionContext {
            cwd: "/repo".to_owned(),
            env: HashMap::new(),
            files: Vec::new(),
            consumer: None,
            mode: None,
            input: None,
            previous_runs: vec![TaskRun {
                task_name: "prepare".to_owned(),
                task_type: TaskKind::Action,
                status: TaskRunStatus::Skipped,
                completion: None,
                output: None,
                data: None,
                next_context: None,
                children: Vec::new(),
                reason: Some("not needed".to_owned()),
            }],
        };

        let serialized = serde_json::to_value(ctx).unwrap();

        assert_eq!(
            serialized["previousRuns"][0]["taskName"],
            serde_json::json!("prepare")
        );
        assert!(serialized.get("previous_runs").is_none());
    }
}
