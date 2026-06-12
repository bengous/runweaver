use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow};

use crate::config::{
    ActionFn, ActionResult, ActionTask, CommandArgs, CommandTask, ExecutionContext,
    NextExecutionContext, ParallelTask, PolicyVerdict, RunweaverConfig, SeriesTask, TaskDefinition,
    TaskKind, TaskOutput, TaskRun, TaskRunStatus, ToolDefinition,
};
use crate::toolchain::{ResolveManagedBinaryResult, managed_tool_path_env, resolve_managed_binary};

use super::context::normalize_files;
use super::result::{
    aggregate_task_completion, aggregate_task_output, is_blocking_run, map_task_completion,
};

/// Executes the named task from `config` inside `ctx`, returning the full
/// [`TaskRun`] tree. Policies run first; denied/skipped tasks complete with
/// a reason instead of output. Series steps merge
/// [`NextExecutionContext`](crate::config::NextExecutionContext) mutations
/// into subsequent steps and stop early on blocking runs when fail-fast is
/// set.
pub fn run_task(
    config: &RunweaverConfig,
    task_name: &str,
    ctx: ExecutionContext,
) -> Result<TaskRun> {
    run_named_task(config, task_name, &ctx)
}

fn run_named_task(
    config: &RunweaverConfig,
    task_name: &str,
    ctx: &ExecutionContext,
) -> Result<TaskRun> {
    let task = config
        .tasks
        .get(task_name)
        .ok_or_else(|| anyhow!("Task \"{task_name}\" does not exist."))?;

    if let Some(gated) = apply_policies(config, task_name, task, ctx)? {
        return Ok(gated);
    }

    match task {
        TaskDefinition::Action(task) => Ok(run_action_task(task_name, &task.run, task.kind(), ctx)),
        TaskDefinition::Command(task) => {
            run_command_task(config, task_name, task, task.kind(), ctx)
        }
        TaskDefinition::Series(task) => run_series_task(
            config,
            task_name,
            task.kind(),
            &task.refs,
            task.fail_fast,
            ctx,
        ),
        TaskDefinition::Parallel(task) => {
            run_parallel_task(config, task_name, task.kind(), &task.refs, ctx)
        }
    }
}

trait TaskKindOf {
    fn kind(&self) -> TaskKind;
}

impl TaskKindOf for ActionTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Action
    }
}
impl TaskKindOf for CommandTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Command
    }
}
impl TaskKindOf for SeriesTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Series
    }
}
impl TaskKindOf for ParallelTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Parallel
    }
}

fn apply_policies(
    config: &RunweaverConfig,
    task_name: &str,
    task: &TaskDefinition,
    ctx: &ExecutionContext,
) -> Result<Option<TaskRun>> {
    for policy_ref in task.policies() {
        let gate = config.policies.get(policy_ref).ok_or_else(|| {
            anyhow!("Task \"{task_name}\" references missing policy \"{policy_ref}\".")
        })?;
        match (gate.evaluate)(ctx) {
            PolicyVerdict::Allow => {}
            PolicyVerdict::Skip { reason } => {
                return Ok(Some(TaskRun {
                    task_name: task_name.to_owned(),
                    task_type: task.kind(),
                    status: TaskRunStatus::Skipped,
                    completion: None,
                    output: None,
                    data: None,
                    next_context: None,
                    children: Vec::new(),
                    reason,
                }));
            }
            PolicyVerdict::Deny { reason } => {
                return Ok(Some(TaskRun {
                    task_name: task_name.to_owned(),
                    task_type: task.kind(),
                    status: TaskRunStatus::Denied,
                    completion: None,
                    output: None,
                    data: None,
                    next_context: None,
                    children: Vec::new(),
                    reason: Some(reason),
                }));
            }
        }
    }
    Ok(None)
}

fn run_action_task(
    task_name: &str,
    run: &ActionFn,
    task_type: TaskKind,
    ctx: &ExecutionContext,
) -> TaskRun {
    match run(ctx) {
        ActionResult::Completed {
            completion,
            output,
            data,
            next_context,
        } => TaskRun {
            task_name: task_name.to_owned(),
            task_type,
            status: TaskRunStatus::Completed,
            completion: Some(completion),
            output: Some(output),
            data,
            next_context: next_context.map(|context| *context),
            children: Vec::new(),
            reason: None,
        },
        ActionResult::Skipped { reason } => TaskRun {
            task_name: task_name.to_owned(),
            task_type,
            status: TaskRunStatus::Skipped,
            completion: None,
            output: None,
            data: None,
            next_context: None,
            children: Vec::new(),
            reason,
        },
        ActionResult::Denied { reason } => TaskRun {
            task_name: task_name.to_owned(),
            task_type,
            status: TaskRunStatus::Denied,
            completion: None,
            output: None,
            data: None,
            next_context: None,
            children: Vec::new(),
            reason: Some(reason),
        },
    }
}

fn run_command_task(
    config: &RunweaverConfig,
    task_name: &str,
    task: &CommandTask,
    task_type: TaskKind,
    ctx: &ExecutionContext,
) -> Result<TaskRun> {
    let tool = config.tools.get(&task.tool).ok_or_else(|| {
        anyhow!(
            "Task \"{task_name}\" references missing tool \"{}\".",
            task.tool
        )
    })?;
    let output = match resolve_executable(&ctx.cwd, tool) {
        Ok(path) => {
            let mut args = tool_config_args(tool);
            args.extend(match &task.args {
                CommandArgs::Static(args) => args.clone(),
                CommandArgs::Dynamic(args) => args(ctx),
            });
            spawn_command(&path, &args, ctx)
        }
        Err(error) => TaskOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error.to_string()),
        },
    };
    let completion = map_task_completion(&output, task.result.as_ref());
    Ok(TaskRun {
        task_name: task_name.to_owned(),
        task_type,
        status: TaskRunStatus::Completed,
        completion: Some(completion),
        output: Some(output),
        data: None,
        next_context: None,
        children: Vec::new(),
        reason: None,
    })
}

fn run_series_task(
    config: &RunweaverConfig,
    task_name: &str,
    task_type: TaskKind,
    refs: &[String],
    fail_fast: bool,
    ctx: &ExecutionContext,
) -> Result<TaskRun> {
    let mut children = Vec::new();
    let mut current_ctx = ctx.clone();
    let mut next_context = None;

    for task_ref in refs {
        let run = run_named_task(config, task_ref, &current_ctx)?;
        children.push(run);
        let previous_runs = {
            let mut previous = ctx.previous_runs.clone();
            previous.extend(children.clone());
            previous
        };
        let child_patch = children.last().and_then(|run| {
            if run.status == TaskRunStatus::Completed {
                run.next_context.clone()
            } else {
                None
            }
        });
        current_ctx = merge_next_context(&current_ctx, child_patch.as_ref(), previous_runs);
        if next_context.is_some() || child_patch.is_some() {
            next_context = next_context_from_base(ctx, &current_ctx);
        }
        if fail_fast && is_blocking_run(children.last().expect("child just pushed")) {
            break;
        }
    }

    let completion = aggregate_task_completion(&children);
    let output = aggregate_task_output(&children, completion);
    Ok(TaskRun {
        task_name: task_name.to_owned(),
        task_type,
        status: TaskRunStatus::Completed,
        completion: Some(completion),
        output: Some(output),
        data: None,
        next_context,
        children,
        reason: None,
    })
}

fn run_parallel_task(
    config: &RunweaverConfig,
    task_name: &str,
    task_type: TaskKind,
    refs: &[String],
    ctx: &ExecutionContext,
) -> Result<TaskRun> {
    let mut children = Vec::with_capacity(refs.len());
    for task_ref in refs {
        children.push(run_named_task(config, task_ref, ctx)?);
    }
    let completion = aggregate_task_completion(&children);
    let output = aggregate_task_output(&children, completion);
    Ok(TaskRun {
        task_name: task_name.to_owned(),
        task_type,
        status: TaskRunStatus::Completed,
        completion: Some(completion),
        output: Some(output),
        data: None,
        next_context: None,
        children,
        reason: None,
    })
}

fn resolve_executable(cwd: &str, tool: &ToolDefinition) -> Result<PathBuf> {
    match tool {
        ToolDefinition::HostCommand(definition) => Ok(PathBuf::from(&definition.program)),
        ToolDefinition::Tool(definition) => {
            match resolve_managed_binary(Path::new(cwd), &definition.program) {
                ResolveManagedBinaryResult::Found { path } => Ok(path),
                ResolveManagedBinaryResult::Missing { diagnostic } => {
                    Err(anyhow!(diagnostic.message))
                }
            }
        }
    }
}

fn tool_config_args(tool: &ToolDefinition) -> Vec<String> {
    match tool {
        ToolDefinition::Tool(definition) => {
            let Some(config) = &definition.config else {
                return Vec::new();
            };
            vec![config.flag.clone(), config.path.clone()]
        }
        ToolDefinition::HostCommand(_) => Vec::new(),
    }
}

fn spawn_command(executable: &Path, args: &[String], ctx: &ExecutionContext) -> TaskOutput {
    let mut command = Command::new(executable);
    command.args(args).current_dir(&ctx.cwd).env_clear();
    for (key, value) in &ctx.env {
        command.env(key, value);
    }
    let parent_path = ctx.env.get("PATH").map(String::as_str);
    command.env(
        "PATH",
        managed_tool_path_env(Path::new(&ctx.cwd), parent_path),
    );

    match command.output() {
        Ok(output) => TaskOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            error: None,
        },
        Err(error) => TaskOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(format!(
                "failed to spawn {}: {error}",
                executable
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("command")
            )),
        },
    }
}

fn merge_next_context(
    ctx: &ExecutionContext,
    next_context: Option<&NextExecutionContext>,
    previous_runs: Vec<TaskRun>,
) -> ExecutionContext {
    let cwd = next_context
        .and_then(|patch| patch.cwd.clone())
        .unwrap_or_else(|| ctx.cwd.clone());
    let files = next_context
        .and_then(|patch| patch.files.clone())
        .unwrap_or_else(|| ctx.files.clone());
    ExecutionContext {
        cwd: cwd.clone(),
        env: next_context
            .and_then(|patch| patch.env.clone())
            .unwrap_or_else(|| ctx.env.clone()),
        files: normalize_files(&cwd, &files),
        consumer: next_context
            .and_then(|patch| patch.consumer.clone())
            .or_else(|| ctx.consumer.clone()),
        mode: next_context
            .and_then(|patch| patch.mode.clone())
            .or_else(|| ctx.mode.clone()),
        input: next_context
            .and_then(|patch| patch.input.clone())
            .or_else(|| ctx.input.clone()),
        previous_runs,
    }
}

fn next_context_from_base(
    base: &ExecutionContext,
    current: &ExecutionContext,
) -> Option<NextExecutionContext> {
    let patch = NextExecutionContext {
        cwd: (current.cwd != base.cwd).then(|| current.cwd.clone()),
        env: (current.env != base.env).then(|| current.env.clone()),
        files: (current.files != base.files).then(|| current.files.clone()),
        consumer: (current.consumer != base.consumer)
            .then(|| current.consumer.clone())
            .flatten(),
        mode: (current.mode != base.mode)
            .then(|| current.mode.clone())
            .flatten(),
        input: (current.input != base.input)
            .then(|| current.input.clone())
            .flatten(),
    };
    if patch.cwd.is_none()
        && patch.env.is_none()
        && patch.files.is_none()
        && patch.consumer.is_none()
        && patch.mode.is_none()
        && patch.input.is_none()
    {
        None
    } else {
        Some(patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::host_command;
    use std::collections::HashMap;

    use crate::config::{ExitCodeRule, ResultMapping, TaskCompletion};

    fn ok_action(_: &ExecutionContext) -> ActionResult {
        ActionResult::success()
    }

    fn denied_action(_: &ExecutionContext) -> ActionResult {
        ActionResult::Denied {
            reason: "blocked".to_owned(),
        }
    }

    #[test]
    fn series_fail_fast_stops_after_blocking_child() {
        let mut config = RunweaverConfig::new();
        config.tasks.insert(
            "a".to_owned(),
            TaskDefinition::Action(ActionTask::new(denied_action)),
        );
        config.tasks.insert(
            "b".to_owned(),
            TaskDefinition::Action(ActionTask::new(ok_action)),
        );
        config.tasks.insert(
            "root".to_owned(),
            TaskDefinition::Series(SeriesTask {
                refs: vec!["a".to_owned(), "b".to_owned()],
                fail_fast: true,
                policies: Vec::new(),
            }),
        );
        let run = run_task(&config, "root", ExecutionContext::new(".")).unwrap();
        assert_eq!(run.completion, Some(TaskCompletion::Error));
        assert_eq!(run.children.len(), 1);
    }

    #[test]
    fn parallel_aggregates_tool_error_before_error() {
        fn tool_error(_: &ExecutionContext) -> ActionResult {
            ActionResult::Completed {
                completion: TaskCompletion::ToolError,
                output: TaskOutput::success(),
                data: None,
                next_context: None,
            }
        }
        let mut config = RunweaverConfig::new();
        config.tasks.insert(
            "a".to_owned(),
            TaskDefinition::Action(ActionTask::new(denied_action)),
        );
        config.tasks.insert(
            "b".to_owned(),
            TaskDefinition::Action(ActionTask::new(tool_error)),
        );
        config.tasks.insert(
            "root".to_owned(),
            TaskDefinition::Parallel(ParallelTask {
                refs: vec!["a".to_owned(), "b".to_owned()],
                fail_fast: false,
                policies: Vec::new(),
            }),
        );
        let run = run_task(&config, "root", ExecutionContext::new(".")).unwrap();
        assert_eq!(run.completion, Some(TaskCompletion::ToolError));
        assert_eq!(run.children.len(), 2);
    }

    #[test]
    fn command_spawn_maps_exit_codes() {
        let mut config = RunweaverConfig::new();
        config.tools.insert("sh".to_owned(), host_command("sh"));
        config.tasks.insert(
            "root".to_owned(),
            TaskDefinition::Command(CommandTask {
                tool: "sh".to_owned(),
                args: CommandArgs::Static(vec!["-c".to_owned(), "exit 2".to_owned()]),
                result: Some(ResultMapping {
                    success: Some(vec![0]),
                    warning: Some(vec![2]),
                    error: ExitCodeRule::Otherwise,
                    tool_error: ExitCodeRule::Unset,
                }),
                policies: Vec::new(),
            }),
        );
        let mut env = HashMap::new();
        env.insert("PATH".to_owned(), std::env::var("PATH").unwrap_or_default());
        let run = run_task(&config, "root", ExecutionContext::new(".").with_env(env)).unwrap();
        assert_eq!(run.completion, Some(TaskCompletion::Warning));
    }
}
