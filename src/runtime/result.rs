use crate::config::{
    ExitCodeRule, ResultMapping, TaskCompletion, TaskOutput, TaskRun, TaskRunStatus,
};

pub fn map_task_completion(output: &TaskOutput, mapping: Option<&ResultMapping>) -> TaskCompletion {
    let Some(exit_code) = output.exit_code else {
        return TaskCompletion::ToolError;
    };

    if let Some(mapping) = mapping {
        if includes_exit_code(mapping.success.as_deref(), exit_code) {
            return TaskCompletion::Success;
        }
        if includes_exit_code(mapping.warning.as_deref(), exit_code) {
            return TaskCompletion::Warning;
        }
        if matches_rule(&mapping.error, exit_code) {
            return TaskCompletion::Error;
        }
        if matches_rule(&mapping.tool_error, exit_code) {
            return TaskCompletion::ToolError;
        }
        if mapping.error == ExitCodeRule::Otherwise {
            return TaskCompletion::Error;
        }
        if mapping.tool_error == ExitCodeRule::Otherwise {
            return TaskCompletion::ToolError;
        }
        return if exit_code == 0 {
            TaskCompletion::Success
        } else {
            TaskCompletion::Error
        };
    }

    if exit_code == 0 {
        TaskCompletion::Success
    } else {
        TaskCompletion::Error
    }
}

/// True when a run should fail a pipeline or block a hook: it was denied,
/// or completed with `Error`/`ToolError`.
pub fn is_blocking_run(run: &TaskRun) -> bool {
    run.status == TaskRunStatus::Denied
        || (run.status == TaskRunStatus::Completed
            && matches!(
                run.completion,
                Some(TaskCompletion::Error | TaskCompletion::ToolError)
            ))
}

/// Folds child completions into one, by severity: tool error > error >
/// warning > success.
pub fn aggregate_task_completion(runs: &[TaskRun]) -> TaskCompletion {
    if runs.iter().any(|run| {
        run.status == TaskRunStatus::Completed && run.completion == Some(TaskCompletion::ToolError)
    }) {
        return TaskCompletion::ToolError;
    }
    if runs.iter().any(|run| {
        run.status == TaskRunStatus::Denied
            || (run.status == TaskRunStatus::Completed
                && run.completion == Some(TaskCompletion::Error))
    }) {
        return TaskCompletion::Error;
    }
    if runs.iter().any(|run| {
        run.status == TaskRunStatus::Completed && run.completion == Some(TaskCompletion::Warning)
    }) {
        return TaskCompletion::Warning;
    }
    TaskCompletion::Success
}

pub fn aggregate_task_output(runs: &[TaskRun], completion: TaskCompletion) -> TaskOutput {
    let stdout = runs
        .iter()
        .filter_map(|run| run.output.as_ref())
        .map(|output| output.stdout.as_str())
        .filter(|text| !text.is_empty())
        .collect::<String>();
    let stderr = runs
        .iter()
        .filter_map(|run| run.output.as_ref())
        .map(|output| output.stderr.as_str())
        .filter(|text| !text.is_empty())
        .collect::<String>();
    let denied_reasons = runs
        .iter()
        .filter(|run| run.status == TaskRunStatus::Denied)
        .filter_map(|run| run.reason.as_deref())
        .collect::<Vec<_>>();
    TaskOutput {
        exit_code: Some(
            if matches!(
                completion,
                TaskCompletion::Success | TaskCompletion::Warning
            ) {
                0
            } else {
                1
            },
        ),
        stdout,
        stderr,
        error: if denied_reasons.is_empty() {
            None
        } else {
            Some(denied_reasons.join("\n"))
        },
    }
}

fn includes_exit_code(rule: Option<&[i32]>, exit_code: i32) -> bool {
    rule.is_some_and(|codes| codes.contains(&exit_code))
}

fn matches_rule(rule: &ExitCodeRule, exit_code: i32) -> bool {
    match rule {
        ExitCodeRule::Codes(codes) => codes.contains(&exit_code),
        ExitCodeRule::Otherwise | ExitCodeRule::Unset => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandArgs, CommandTask, TaskDefinition, TaskKind};

    #[test]
    fn map_task_completion_uses_explicit_warning_exit_codes() {
        let mapping = ResultMapping {
            success: Some(vec![0]),
            warning: Some(vec![2]),
            error: ExitCodeRule::Otherwise,
            tool_error: ExitCodeRule::Unset,
        };
        let output = TaskOutput {
            exit_code: Some(2),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
        };

        assert_eq!(
            map_task_completion(&output, Some(&mapping)),
            TaskCompletion::Warning
        );
    }

    #[test]
    fn aggregate_task_completion_prefers_tool_error_over_error() {
        let runs = vec![
            completed_run("error", TaskCompletion::Error),
            completed_run("tool", TaskCompletion::ToolError),
        ];

        assert_eq!(aggregate_task_completion(&runs), TaskCompletion::ToolError);
    }

    #[test]
    fn aggregate_task_output_preserves_denied_reasons_as_error_text() {
        let run = TaskRun {
            task_name: "guard".to_owned(),
            task_type: TaskKind::Command,
            status: TaskRunStatus::Denied,
            completion: None,
            output: None,
            data: None,
            next_context: None,
            children: Vec::new(),
            reason: Some("blocked".to_owned()),
        };

        assert_eq!(
            aggregate_task_output(&[run], TaskCompletion::Error).error,
            Some("blocked".to_owned())
        );
    }

    fn completed_run(task_name: &str, completion: TaskCompletion) -> TaskRun {
        TaskRun {
            task_name: task_name.to_owned(),
            task_type: TaskDefinition::Command(CommandTask {
                tool: "tool".to_owned(),
                args: CommandArgs::Static(Vec::new()),
                result: None,
                policies: Vec::new(),
            })
            .kind(),
            status: TaskRunStatus::Completed,
            completion: Some(completion),
            output: Some(TaskOutput::success()),
            data: None,
            next_context: None,
            children: Vec::new(),
            reason: None,
        }
    }
}
