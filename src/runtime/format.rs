use serde::Serialize;

use crate::config::{TaskCompletion, TaskOutput, TaskRun, TaskRunStatus};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactTaskRun {
    pub task_name: String,
    pub task_type: crate::config::TaskKind,
    pub status: TaskRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<TaskCompletion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<CompactTaskOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<CompactTaskRun>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactTaskOutput {
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Reduces a run tree to the agent-facing view: successful children are
/// dropped and output is kept only where it explains a failure.
pub fn compact_run_for_agents(run: &TaskRun) -> CompactTaskRun {
    let mut compact = CompactTaskRun {
        task_name: run.task_name.clone(),
        task_type: run.task_type,
        status: run.status,
        completion: None,
        output: None,
        reason: None,
        children: Vec::new(),
    };

    if run.status != TaskRunStatus::Completed {
        compact.reason = run.reason.clone();
        return compact;
    }

    compact.completion = run.completion;
    compact.output = run
        .output
        .as_ref()
        .zip(run.completion)
        .map(|(output, completion)| compact_output(output, completion, !run.children.is_empty()));
    compact.children = run
        .children
        .iter()
        .map(compact_run_for_agents)
        .filter(should_keep_compact_run)
        .collect();
    compact
}

/// Renders the denied/skipped/failing runs of a tree as human-readable text.
pub fn format_notable_runs(run: &TaskRun) -> String {
    let mut runs = Vec::new();
    collect_notable_runs(run, &mut runs);
    runs.into_iter().map(format_notable_run).collect()
}

pub fn task_run_result_label(run: &TaskRun) -> &'static str {
    match run.status {
        TaskRunStatus::Completed => run
            .completion
            .map(task_completion_label)
            .unwrap_or("completed"),
        TaskRunStatus::Skipped => "skipped",
        TaskRunStatus::Denied => "denied",
    }
}

fn compact_output(
    output: &TaskOutput,
    completion: TaskCompletion,
    has_children: bool,
) -> CompactTaskOutput {
    if has_children {
        return CompactTaskOutput {
            exit_code: output.exit_code,
            stdout: None,
            stderr: None,
            error: output.error.clone(),
        };
    }
    if completion == TaskCompletion::Success {
        return CompactTaskOutput {
            exit_code: output.exit_code,
            stdout: None,
            stderr: None,
            error: None,
        };
    }
    CompactTaskOutput {
        exit_code: output.exit_code,
        stdout: non_empty(output.stdout.clone()),
        stderr: non_empty(output.stderr.clone()),
        error: output.error.clone(),
    }
}

fn should_keep_compact_run(run: &CompactTaskRun) -> bool {
    run.status != TaskRunStatus::Completed
        || run.completion != Some(TaskCompletion::Success)
        || !run.children.is_empty()
}

fn collect_notable_runs<'a>(run: &'a TaskRun, output: &mut Vec<&'a TaskRun>) {
    if run.status != TaskRunStatus::Completed {
        output.push(run);
        return;
    }
    if !run.children.is_empty() {
        let start_len = output.len();
        for child in &run.children {
            collect_notable_runs(child, output);
        }
        if output.len() == start_len && run.completion != Some(TaskCompletion::Success) {
            output.push(run);
        }
        return;
    }
    if run.completion != Some(TaskCompletion::Success) {
        output.push(run);
    }
}

fn format_notable_run(run: &TaskRun) -> String {
    let mut chunks = vec![format!("{}: {}", run.task_name, task_run_result_label(run))];
    if run.status == TaskRunStatus::Completed {
        if let Some(output) = &run.output {
            push_non_empty(&mut chunks, &output.stdout);
            push_non_empty(&mut chunks, &output.stderr);
            if let Some(error) = &output.error {
                push_non_empty(&mut chunks, error);
            }
        }
    } else if let Some(reason) = &run.reason {
        push_non_empty(&mut chunks, reason);
    }
    format!("{}\n", chunks.join("\n"))
}

fn task_completion_label(completion: TaskCompletion) -> &'static str {
    match completion {
        TaskCompletion::Success => "success",
        TaskCompletion::Warning => "warning",
        TaskCompletion::Error => "error",
        TaskCompletion::ToolError => "toolError",
    }
}

fn push_non_empty(chunks: &mut Vec<String>, text: &str) {
    let trimmed = text.trim_end();
    if !trimmed.is_empty() {
        chunks.push(trimmed.to_owned());
    }
}

fn non_empty(text: String) -> Option<String> {
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TaskKind, TaskOutput};

    #[test]
    fn compact_run_hides_success_output_and_keeps_failing_children() {
        let run = TaskRun {
            task_name: "check".to_owned(),
            task_type: TaskKind::Parallel,
            status: TaskRunStatus::Completed,
            completion: Some(TaskCompletion::Error),
            output: Some(TaskOutput {
                exit_code: Some(1),
                stdout: "parent out\n".to_owned(),
                stderr: "parent err\n".to_owned(),
                error: None,
            }),
            data: None,
            next_context: None,
            children: vec![
                completed_run("ok", TaskCompletion::Success, "hidden\n", ""),
                completed_run(
                    "fail",
                    TaskCompletion::Error,
                    "bad stdout\n",
                    "bad stderr\n",
                ),
            ],
            reason: None,
        };

        let compact = serde_json::to_value(compact_run_for_agents(&run)).unwrap();

        assert_eq!(compact["output"], serde_json::json!({ "exitCode": 1 }));
        assert_eq!(compact["children"][0]["taskName"], "fail");
        assert!(!compact.to_string().contains("hidden"));
    }

    #[test]
    fn notable_runs_include_failures_and_denied_reasons() {
        let denied = TaskRun {
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

        assert_eq!(format_notable_runs(&denied), "guard: denied\nblocked\n");
    }

    fn completed_run(
        task_name: &str,
        completion: TaskCompletion,
        stdout: &str,
        stderr: &str,
    ) -> TaskRun {
        TaskRun {
            task_name: task_name.to_owned(),
            task_type: TaskKind::Command,
            status: TaskRunStatus::Completed,
            completion: Some(completion),
            output: Some(TaskOutput {
                exit_code: Some(if completion == TaskCompletion::Success {
                    0
                } else {
                    1
                }),
                stdout: stdout.to_owned(),
                stderr: stderr.to_owned(),
                error: None,
            }),
            data: None,
            next_context: None,
            children: Vec::new(),
            reason: None,
        }
    }
}
