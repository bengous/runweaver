use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{ExecutionContext, TaskRun, normalize_file_path};

#[derive(Debug, Clone)]
pub struct CreateExecutionContextOptions {
    pub cwd: String,
    pub env: Option<HashMap<String, String>>,
    pub files: Vec<String>,
    pub consumer: Option<String>,
    pub mode: Option<String>,
    pub input: Option<Value>,
    pub previous_runs: Vec<TaskRun>,
}

impl CreateExecutionContextOptions {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self {
            cwd: cwd.into(),
            env: None,
            files: Vec::new(),
            consumer: None,
            mode: None,
            input: None,
            previous_runs: Vec::new(),
        }
    }
}

pub fn create_execution_context(options: CreateExecutionContextOptions) -> ExecutionContext {
    let cwd = absolute_path(Path::new(&options.cwd));
    let cwd_text = path_to_string(&cwd);
    ExecutionContext {
        cwd: cwd_text.clone(),
        env: options.env.unwrap_or_default(),
        files: normalize_files(&cwd, &options.files),
        consumer: options.consumer,
        mode: options.mode,
        input: options.input,
        previous_runs: options.previous_runs,
    }
}

pub fn normalize_files(cwd: impl AsRef<Path>, files: &[String]) -> Vec<String> {
    let root = absolute_path(cwd.as_ref());
    let mut seen = HashMap::new();
    let mut normalized = Vec::new();
    for file in files {
        let raw = file.trim();
        if raw.is_empty() {
            continue;
        }
        let relative = relative_to_root(&root, raw);
        let normalized_file = normalize_file_path(&relative);
        if seen.insert(normalized_file.clone(), ()).is_none() {
            normalized.push(normalized_file);
        }
    }
    normalized
}

fn relative_to_root(root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.strip_prefix(root)
            .ok()
            .and_then(|path| path.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| raw.to_owned())
    } else {
        raw.to_owned()
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TaskCompletion, TaskKind, TaskOutput, TaskRunStatus};

    #[test]
    fn create_execution_context_preserves_structured_input() {
        let mut options = CreateExecutionContextOptions::new(".");
        options.input = Some(serde_json::json!({ "changedFiles": ["src/a.ts"] }));

        let ctx = create_execution_context(options);

        assert_eq!(
            ctx.input,
            Some(serde_json::json!({ "changedFiles": ["src/a.ts"] }))
        );
    }

    #[test]
    fn create_execution_context_accepts_explicit_previous_runs() {
        let previous_run = TaskRun {
            task_name: "prepare".to_owned(),
            task_type: TaskKind::Action,
            status: TaskRunStatus::Completed,
            completion: Some(TaskCompletion::Success),
            output: Some(TaskOutput::success()),
            data: Some(serde_json::json!({ "ready": true })),
            next_context: None,
            children: Vec::new(),
            reason: None,
        };
        let mut options = CreateExecutionContextOptions::new(".");
        options.previous_runs = vec![previous_run.clone()];

        let ctx = create_execution_context(options);

        assert_eq!(ctx.previous_runs, vec![previous_run]);
    }

    #[test]
    fn normalize_files_keeps_unique_posix_paths_relative_to_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let files = vec![
            path_to_string(&cwd.join("src").join("a.ts")),
            "./src/a.ts".to_owned(),
            "src/b.ts".to_owned(),
            " ".to_owned(),
        ];

        assert_eq!(normalize_files(&cwd, &files), vec!["src/a.ts", "src/b.ts"]);
    }
}
