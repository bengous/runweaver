use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;

use crate::config::{
    ExecutionContext, RunweaverConfig, TaskCompletion, TaskOutput, TaskRun, TaskRunStatus,
};
use crate::runtime::{is_blocking_run, run_task};

use super::{HookEvent, HookOutcome, UpdatedFileSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFileSnapshot {
    pub path: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineOutcome {
    Pass,
    Fail {
        diagnostics: Vec<String>,
    },
    Fixed {
        changed_files: Vec<ChangedFileSnapshot>,
        follow_up_failures: Vec<String>,
    },
}

impl PipelineOutcome {
    pub fn to_hook_outcome(self, stage: AgentsPipelineStage) -> HookOutcome {
        match self {
            Self::Pass => HookOutcome::pass(),
            Self::Fail { diagnostics } => HookOutcome::block(format!(
                "{} failed:\n{}",
                stage.label(),
                diagnostics.join("\n\n")
            )),
            Self::Fixed {
                changed_files,
                follow_up_failures,
            } => {
                let updated_file = changed_files.first().map(|file| UpdatedFileSnapshot {
                    path: file.path.clone(),
                    before: file.before.clone(),
                    after: file.after.clone(),
                });
                if follow_up_failures.is_empty() {
                    HookOutcome::Pass {
                        system_message: Some(format_fixed_message(&changed_files)),
                        updated_file,
                    }
                } else {
                    HookOutcome::Block {
                        reason: format!(
                            "{} fixed files, but follow-up checks failed:\n{}",
                            stage.label(),
                            follow_up_failures.join("\n\n")
                        ),
                        system_message: Some(format_fixed_message(&changed_files)),
                        updated_file,
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsPipelineStage {
    PostEdit,
    Stop,
}

impl AgentsPipelineStage {
    fn label(self) -> &'static str {
        match self {
            Self::PostEdit => "Post-edit pipeline",
            Self::Stop => "Stop validation",
        }
    }
}

pub fn run_post_edit_pipeline(
    config: &RunweaverConfig,
    pipeline: &str,
    event: &HookEvent,
) -> Result<HookOutcome> {
    let root = project_root(event);
    let extracted = extract_touched_paths(event, &root);
    record_touched_paths(event, &root, &extracted)?;
    let paths = if extracted.is_empty() {
        read_touched_paths(event, &root)
    } else {
        extracted
    };
    let existing = existing_paths(&root, &paths);
    if existing.is_empty() {
        return Ok(HookOutcome::pass());
    }
    Ok(
        run_pipeline_with_outcome(config, pipeline, &root, &existing)
            .to_hook_outcome(AgentsPipelineStage::PostEdit),
    )
}

pub fn run_stop_pipeline(
    config: &RunweaverConfig,
    pipeline: &str,
    event: &HookEvent,
) -> Result<HookOutcome> {
    if event.stop_hook_active {
        return Ok(HookOutcome::pass());
    }
    let root = project_root(event);
    let extracted = extract_touched_paths(event, &root);
    let stored = read_touched_paths(event, &root);
    let touched = merge_paths(&extracted, &stored);
    let relevant = existing_paths(&root, &touched);
    if relevant.is_empty() {
        clear_touched_paths(event, &root)?;
        return Ok(HookOutcome::pass());
    }

    let before = capture_git_fingerprint(&root)?;
    let outcome = run_pipeline_with_outcome(config, pipeline, &root, &relevant);
    let after = capture_git_fingerprint(&root)?;
    if before.signature != after.signature {
        return Ok(HookOutcome::block(format!(
            "Stop validation changed repository state unexpectedly:\n{}",
            changed_git_paths(&before, &after).join("\n")
        )));
    }
    if matches!(outcome, PipelineOutcome::Pass) {
        clear_touched_paths(event, &root)?;
    }
    Ok(outcome.to_hook_outcome(AgentsPipelineStage::Stop))
}

pub fn run_pipeline_with_outcome(
    config: &RunweaverConfig,
    pipeline: &str,
    root: &Path,
    files: &[String],
) -> PipelineOutcome {
    let before = read_file_snapshots(root, files);
    let run = match run_task(
        config,
        pipeline,
        ExecutionContext::new(root.to_string_lossy())
            .with_files(files.to_vec())
            .with_env(std::env::vars().collect()),
    ) {
        Ok(run) => run,
        Err(error) => {
            return PipelineOutcome::Fail {
                diagnostics: vec![error.to_string()],
            };
        }
    };
    let changed_files = changed_files(root, before);
    let failures = blocking_run_messages(&run);
    if changed_files.is_empty() && failures.is_empty() {
        PipelineOutcome::Pass
    } else if changed_files.is_empty() {
        PipelineOutcome::Fail {
            diagnostics: failures,
        }
    } else {
        PipelineOutcome::Fixed {
            changed_files,
            follow_up_failures: failures,
        }
    }
}

fn project_root(event: &HookEvent) -> PathBuf {
    normalize_lexically(Path::new(&event.cwd))
}

fn extract_touched_paths(event: &HookEvent, root: &Path) -> Vec<String> {
    let cwd = normalize_lexically(Path::new(&event.cwd));
    let mut candidates = Vec::new();
    if let Some(patch) = event.patch_text.as_deref() {
        candidates.extend(extract_apply_patch_paths(patch));
    }
    candidates.extend(event.touched_path_candidates.iter().cloned());

    let mut paths = BTreeSet::new();
    for candidate in candidates {
        if let Some(path) = normalize_project_path(&candidate, root, &cwd) {
            paths.insert(path);
        }
    }
    paths.into_iter().collect()
}

fn extract_apply_patch_paths(patch: &str) -> Vec<String> {
    let prefixes = [
        "*** Add File: ",
        "*** Update File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    let mut paths = BTreeSet::new();
    for line in patch.lines() {
        for prefix in prefixes {
            if let Some(rest) = line.strip_prefix(prefix) {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    paths.insert(trimmed.to_owned());
                }
            }
        }
    }
    paths.into_iter().collect()
}

fn normalize_project_path(file_path: &str, root: &Path, cwd: &Path) -> Option<String> {
    if file_path.trim().is_empty() {
        return None;
    }
    let path = Path::new(file_path);
    let absolute = if path.is_absolute() {
        normalize_lexically(path)
    } else {
        normalize_lexically(&cwd.join(path))
    };
    let relative = absolute.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(path_to_posix(relative))
}

fn record_touched_paths(event: &HookEvent, root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    for path in touched_state_paths(event, root) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut next: BTreeSet<String> = read_touched_path_file(&path).into_iter().collect();
        next.extend(paths.iter().cloned());
        fs::write(path, format!("{}\n", serde_json::to_string_pretty(&next)?))?;
    }
    Ok(())
}

fn read_touched_paths(event: &HookEvent, root: &Path) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for path in touched_state_paths(event, root) {
        paths.extend(read_touched_path_file(&path));
    }
    paths.into_iter().collect()
}

fn clear_touched_paths(event: &HookEvent, root: &Path) -> Result<()> {
    for path in touched_state_paths(event, root) {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn touched_state_paths(event: &HookEvent, root: &Path) -> Vec<PathBuf> {
    let event_key = event.tool_call_id.as_deref().unwrap_or("pending");
    let repo_name = sanitize_state_key(&root.to_string_lossy());
    let identity = format!(
        "{}-{}",
        sanitize_state_key(&event.harness),
        sanitize_state_key(&event.session_id)
    );
    let base = std::env::temp_dir()
        .join(format!("{repo_name}-agent-feedback"))
        .join(identity);
    let mut paths = vec![base.join(format!("{}.json", sanitize_state_key(event_key)))];
    let pending = base.join("pending.json");
    if pending != paths[0] {
        paths.push(pending);
    }
    paths
}

fn read_touched_path_file(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<BTreeSet<String>>(&text)
        .map(|paths| paths.into_iter().collect())
        .unwrap_or_default()
}

fn read_file_snapshots(root: &Path, files: &[String]) -> Vec<ChangedFileSnapshot> {
    files
        .iter()
        .filter_map(|file| {
            fs::read_to_string(root.join(file))
                .ok()
                .map(|before| ChangedFileSnapshot {
                    path: file.clone(),
                    before,
                    after: String::new(),
                })
        })
        .collect()
}

fn changed_files(root: &Path, before: Vec<ChangedFileSnapshot>) -> Vec<ChangedFileSnapshot> {
    before
        .into_iter()
        .filter_map(|mut file| {
            let after = fs::read_to_string(root.join(&file.path)).ok()?;
            if after == file.before {
                return None;
            }
            file.after = after;
            Some(file)
        })
        .collect()
}

fn existing_paths(root: &Path, paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| root.join(path).exists())
        .cloned()
        .collect()
}

fn merge_paths(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn blocking_run_messages(run: &TaskRun) -> Vec<String> {
    collect_blocking_runs(run)
        .into_iter()
        .map(format_task_run_failure)
        .filter(|message| !message.is_empty())
        .collect()
}

fn collect_blocking_runs(run: &TaskRun) -> Vec<&TaskRun> {
    if !run.children.is_empty() {
        let children = run
            .children
            .iter()
            .flat_map(collect_blocking_runs)
            .collect::<Vec<_>>();
        if !children.is_empty() || !is_blocking_run(run) {
            return children;
        }
    }
    if is_blocking_run(run) {
        vec![run]
    } else {
        Vec::new()
    }
}

fn format_task_run_failure(run: &TaskRun) -> String {
    match run.status {
        TaskRunStatus::Denied => format!(
            "{}: denied\n{}",
            run.task_name,
            run.reason.as_deref().unwrap_or_default()
        ),
        TaskRunStatus::Skipped => String::new(),
        TaskRunStatus::Completed => format!(
            "{}: {:?}\n{}",
            run.task_name,
            run.completion.unwrap_or(TaskCompletion::ToolError),
            format_task_output(run.output.as_ref())
        ),
    }
}

fn format_task_output(output: Option<&TaskOutput>) -> String {
    let Some(output) = output else {
        return "Tool failed before producing output.".to_owned();
    };
    let detail = tail(
        &format!(
            "{}\n{}\n{}",
            output.stdout,
            output.stderr,
            output.error.as_deref().unwrap_or_default()
        ),
        40,
    );
    if !detail.is_empty() {
        detail
    } else {
        match output.exit_code {
            Some(code) => format!("exit {code}"),
            None => "Tool failed before producing output.".to_owned(),
        }
    }
}

fn tail(text: &str, lines: usize) -> String {
    let mut kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if kept.len() > lines {
        kept = kept.split_off(kept.len() - lines);
    }
    kept.join("\n")
}

fn format_fixed_message(files: &[ChangedFileSnapshot]) -> String {
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    format!("Updated files: {}", paths.join(", "))
}

#[derive(Debug, Clone)]
struct GitFingerprint {
    signature: String,
    paths: Vec<String>,
}

fn capture_git_fingerprint(root: &Path) -> Result<GitFingerprint> {
    let status = run_command(root, "git", &["status", "--porcelain=v1", "-z", "-uall"])?;
    let worktree_diff = run_command(root, "git", &["diff", "--no-ext-diff", "--binary"])?;
    let index_diff = run_command(
        root,
        "git",
        &["diff", "--cached", "--no-ext-diff", "--binary"],
    )?;
    let paths = parse_status_paths(&status);
    let signature = serde_json::json!({
        "status": status,
        "worktreeDiff": worktree_diff,
        "indexDiff": index_diff,
    })
    .to_string();
    Ok(GitFingerprint { signature, paths })
}

fn run_command(root: &Path, executable: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(executable)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "{} failed: {}",
            executable,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_status_paths(status: &str) -> Vec<String> {
    status
        .split('\0')
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry[3..].to_owned())
        .collect()
}

fn changed_git_paths(before: &GitFingerprint, after: &GitFingerprint) -> Vec<String> {
    before
        .paths
        .iter()
        .chain(after.paths.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sanitize_state_key(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(component.as_os_str()),
            Component::Normal(segment) => result.push(segment),
        }
    }
    result
}

fn path_to_posix(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_outcome_pass_projects_to_hook_pass() {
        assert_eq!(
            PipelineOutcome::Pass.to_hook_outcome(AgentsPipelineStage::Stop),
            HookOutcome::pass()
        );
    }

    #[test]
    fn pipeline_outcome_fail_projects_to_block_with_diagnostics() {
        let outcome = PipelineOutcome::Fail {
            diagnostics: vec!["lint failed".to_owned(), "test failed".to_owned()],
        }
        .to_hook_outcome(AgentsPipelineStage::PostEdit);

        assert!(matches!(
            outcome,
            HookOutcome::Block { reason, .. }
                if reason.contains("Post-edit pipeline failed")
                    && reason.contains("lint failed")
                    && reason.contains("test failed")
        ));
    }

    #[test]
    fn pipeline_outcome_fixed_projects_changed_file_snapshot() {
        let outcome = PipelineOutcome::Fixed {
            changed_files: vec![ChangedFileSnapshot {
                path: "src/lib.ts".to_owned(),
                before: "export const ok=true;\n".to_owned(),
                after: "export const ok = true;\n".to_owned(),
            }],
            follow_up_failures: Vec::new(),
        }
        .to_hook_outcome(AgentsPipelineStage::PostEdit);

        assert!(matches!(
            outcome,
            HookOutcome::Pass {
                system_message: Some(message),
                updated_file: Some(UpdatedFileSnapshot { path, before, after })
            } if message == "Updated files: src/lib.ts"
                && path == "src/lib.ts"
                && before == "export const ok=true;\n"
                && after == "export const ok = true;\n"
        ));
    }

    #[test]
    fn pipeline_outcome_fixed_with_follow_up_failures_blocks_and_keeps_snapshot() {
        let outcome = PipelineOutcome::Fixed {
            changed_files: vec![ChangedFileSnapshot {
                path: "src/lib.ts".to_owned(),
                before: "bad".to_owned(),
                after: "better".to_owned(),
            }],
            follow_up_failures: vec!["typecheck failed".to_owned()],
        }
        .to_hook_outcome(AgentsPipelineStage::PostEdit);

        assert!(matches!(
            outcome,
            HookOutcome::Block {
                reason,
                updated_file: Some(UpdatedFileSnapshot { path, .. }),
                ..
            } if reason.contains("follow-up checks failed")
                && reason.contains("typecheck failed")
                && path == "src/lib.ts"
        ));
    }
}
