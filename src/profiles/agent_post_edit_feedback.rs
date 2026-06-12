use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Profile, ProfileError, define_profile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPostEditFeedbackInput {
    pub cwd: String,
    pub session_id: String,
    #[serde(default)]
    pub touched_path_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPostEditUpdatedFile {
    pub path: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPostEditFeedbackResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_file: Option<AgentPostEditUpdatedFile>,
}

impl AgentPostEditFeedbackResult {
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            block_reason: Some(reason.into()),
            updated_file: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentPostEditFeedbackCheckResult {
    Passed,
    Blocked { block_reason: String },
}

impl AgentPostEditFeedbackCheckResult {
    pub fn passed() -> Self {
        Self::Passed
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            block_reason: reason.into(),
        }
    }

    fn block_reason(self) -> Option<String> {
        match self {
            Self::Passed => None,
            Self::Blocked { block_reason } if block_reason.is_empty() => None,
            Self::Blocked { block_reason } => Some(block_reason),
        }
    }
}

pub trait AgentPostEditFeedbackPorts: Send + Sync {
    fn extract_touched_paths(
        &self,
        input: &AgentPostEditFeedbackInput,
    ) -> Result<Vec<String>, ProfileError>;

    fn record_touched_paths(
        &self,
        input: &AgentPostEditFeedbackInput,
        paths: &[String],
    ) -> Result<(), ProfileError>;

    fn read_touched_paths(
        &self,
        input: &AgentPostEditFeedbackInput,
    ) -> Result<Vec<String>, ProfileError>;

    fn normalize_path(
        &self,
        input: &AgentPostEditFeedbackInput,
        path: &str,
    ) -> Result<Option<String>, ProfileError>;

    fn is_inside_project(
        &self,
        input: &AgentPostEditFeedbackInput,
        normalized_path: &str,
    ) -> Result<bool, ProfileError>;

    fn file_exists(
        &self,
        input: &AgentPostEditFeedbackInput,
        normalized_path: &str,
    ) -> Result<bool, ProfileError>;

    fn read_text(
        &self,
        input: &AgentPostEditFeedbackInput,
        normalized_path: &str,
    ) -> Result<Option<String>, ProfileError>;

    fn generated_guard(
        &self,
        input: &AgentPostEditFeedbackInput,
        paths: &[String],
    ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError>;

    fn run_operation(
        &self,
        input: &AgentPostEditFeedbackInput,
        existing_paths: &[String],
    ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError>;
}

#[derive(Clone)]
pub struct AgentPostEditFeedbackProfileOptions {
    pub ports: Arc<dyn AgentPostEditFeedbackPorts>,
    pub name: Option<String>,
}

impl AgentPostEditFeedbackProfileOptions {
    pub fn new(ports: Arc<dyn AgentPostEditFeedbackPorts>) -> Self {
        Self { ports, name: None }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

impl std::fmt::Debug for AgentPostEditFeedbackProfileOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentPostEditFeedbackProfileOptions")
            .field("ports", &"<ports>")
            .field("name", &self.name)
            .finish()
    }
}

pub fn run_agent_post_edit_feedback(
    input: &AgentPostEditFeedbackInput,
    ports: &dyn AgentPostEditFeedbackPorts,
) -> Result<AgentPostEditFeedbackResult, ProfileError> {
    let extracted_paths = ports.extract_touched_paths(input)?;
    ports.record_touched_paths(input, &extracted_paths)?;

    let candidate_paths = if extracted_paths.is_empty() {
        ports.read_touched_paths(input)?
    } else {
        extracted_paths
    };
    let existing_paths = existing_project_paths(input, &candidate_paths, ports)?;

    let guard_result = match ports.generated_guard(input, &existing_paths) {
        Ok(result) => result,
        Err(error) => return Ok(AgentPostEditFeedbackResult::block(error.message)),
    };
    if let Some(block_reason) = guard_result.block_reason() {
        return Ok(AgentPostEditFeedbackResult::block(block_reason));
    }

    if existing_paths.is_empty() {
        return Ok(AgentPostEditFeedbackResult::default());
    }

    let capture_path = single_path(&existing_paths);
    let before = match capture_path {
        Some(path) => ports.read_text(input, path)?,
        None => None,
    };

    let operation_result = match ports.run_operation(input, &existing_paths) {
        Ok(result) => result,
        Err(error) => {
            let updated_file = changed_single_file_snapshot(input, ports, capture_path, before)?;
            return Ok(AgentPostEditFeedbackResult {
                block_reason: Some(error.message),
                updated_file,
            });
        }
    };

    let updated_file = changed_single_file_snapshot(input, ports, capture_path, before)?;
    if let Some(block_reason) = operation_result.block_reason() {
        return Ok(AgentPostEditFeedbackResult {
            block_reason: Some(block_reason),
            updated_file,
        });
    }

    Ok(AgentPostEditFeedbackResult {
        block_reason: None,
        updated_file,
    })
}

/// Builds the post-edit feedback [`Profile`]: filters touched paths to
/// in-project files, applies the generated-file guard, records touched
/// paths for later stop validation, and runs the feedback operation —
/// optionally returning updated file content.
pub fn agent_post_edit_feedback_profile(options: AgentPostEditFeedbackProfileOptions) -> Profile {
    let name = options
        .name
        .unwrap_or_else(|| "agent-post-edit-feedback".to_owned());
    let after_ports = Arc::clone(&options.ports);
    let error_ports = Arc::clone(&options.ports);

    define_profile(
        Profile::new(name)
            .after_operation(move |output, _context, input| {
                let base = feedback_result_from_value(output)?;
                let feedback_input = feedback_input_from_value(input)?;
                let feedback = run_agent_post_edit_feedback(&feedback_input, after_ports.as_ref())?;
                feedback_result_to_value(merge_feedback(base, feedback))
            })
            .on_operation_error(move |error, _context, input| {
                let feedback_input = feedback_input_from_value(input)?;
                let feedback = run_agent_post_edit_feedback(&feedback_input, error_ports.as_ref())?;
                feedback_result_to_value(merge_feedback(
                    AgentPostEditFeedbackResult::block(error),
                    feedback,
                ))
            }),
    )
}

fn existing_project_paths(
    input: &AgentPostEditFeedbackInput,
    paths: &[String],
    ports: &dyn AgentPostEditFeedbackPorts,
) -> Result<Vec<String>, ProfileError> {
    let mut existing_paths = Vec::new();
    for path in paths {
        let Some(normalized_path) = ports.normalize_path(input, path)? else {
            continue;
        };
        if normalized_path.is_empty() {
            continue;
        }
        if !ports.is_inside_project(input, &normalized_path)? {
            continue;
        }
        if ports.file_exists(input, &normalized_path)? {
            existing_paths.push(normalized_path);
        }
    }
    Ok(existing_paths)
}

fn changed_single_file_snapshot(
    input: &AgentPostEditFeedbackInput,
    ports: &dyn AgentPostEditFeedbackPorts,
    path: Option<&str>,
    before: Option<String>,
) -> Result<Option<AgentPostEditUpdatedFile>, ProfileError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(before) = before else {
        return Ok(None);
    };

    let Some(after) = ports.read_text(input, path)? else {
        return Ok(None);
    };
    if after == before {
        return Ok(None);
    }

    Ok(Some(AgentPostEditUpdatedFile {
        path: path.to_owned(),
        before,
        after,
    }))
}

fn single_path(paths: &[String]) -> Option<&str> {
    match paths {
        [path] => Some(path.as_str()),
        _ => None,
    }
}

fn feedback_input_from_value(value: &Value) -> Result<AgentPostEditFeedbackInput, ProfileError> {
    serde_json::from_value(value.clone()).map_err(|error| {
        ProfileError::new(format!("Invalid agent post-edit feedback input: {error}"))
    })
}

fn feedback_result_from_value(value: Value) -> Result<AgentPostEditFeedbackResult, ProfileError> {
    serde_json::from_value(value).map_err(|error| {
        ProfileError::new(format!("Invalid agent post-edit feedback result: {error}"))
    })
}

fn feedback_result_to_value(result: AgentPostEditFeedbackResult) -> Result<Value, ProfileError> {
    serde_json::to_value(result).map_err(|error| {
        ProfileError::new(format!("Invalid agent post-edit feedback result: {error}"))
    })
}

fn merge_feedback(
    base: AgentPostEditFeedbackResult,
    feedback: AgentPostEditFeedbackResult,
) -> AgentPostEditFeedbackResult {
    AgentPostEditFeedbackResult {
        block_reason: feedback.block_reason.or(base.block_reason),
        updated_file: feedback.updated_file.or(base.updated_file),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;

    type Files = Arc<Mutex<BTreeMap<String, String>>>;
    type ExtractFn =
        dyn Fn(&AgentPostEditFeedbackInput) -> Result<Vec<String>, ProfileError> + Send + Sync;
    type NormalizeFn = dyn Fn(&AgentPostEditFeedbackInput, &str) -> Result<Option<String>, ProfileError>
        + Send
        + Sync;
    type GuardFn = dyn Fn(
            &AgentPostEditFeedbackInput,
            &[String],
        ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError>
        + Send
        + Sync;
    type OperationFn = dyn Fn(
            &AgentPostEditFeedbackInput,
            &[String],
        ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError>
        + Send
        + Sync;

    struct MemoryPorts {
        files: Files,
        stored_paths: Vec<String>,
        recorded: Mutex<Vec<Vec<String>>>,
        operations: Mutex<Vec<Vec<String>>>,
        extract_touched_paths: Option<Arc<ExtractFn>>,
        normalize_path: Option<Arc<NormalizeFn>>,
        generated_guard: Option<Arc<GuardFn>>,
        run_operation: Option<Arc<OperationFn>>,
    }

    impl MemoryPorts {
        fn new(files: Files) -> Self {
            Self {
                files,
                stored_paths: Vec::new(),
                recorded: Mutex::new(Vec::new()),
                operations: Mutex::new(Vec::new()),
                extract_touched_paths: None,
                normalize_path: None,
                generated_guard: None,
                run_operation: None,
            }
        }

        fn with_stored_paths(mut self, paths: impl IntoIterator<Item = &'static str>) -> Self {
            self.stored_paths = paths.into_iter().map(str::to_owned).collect();
            self
        }

        fn with_extract_touched_paths(
            mut self,
            extract_touched_paths: impl Fn(
                &AgentPostEditFeedbackInput,
            ) -> Result<Vec<String>, ProfileError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            self.extract_touched_paths = Some(Arc::new(extract_touched_paths));
            self
        }

        fn with_normalize_path(
            mut self,
            normalize_path: impl Fn(
                &AgentPostEditFeedbackInput,
                &str,
            ) -> Result<Option<String>, ProfileError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            self.normalize_path = Some(Arc::new(normalize_path));
            self
        }

        fn with_generated_guard(
            mut self,
            generated_guard: impl Fn(
                &AgentPostEditFeedbackInput,
                &[String],
            )
                -> Result<AgentPostEditFeedbackCheckResult, ProfileError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            self.generated_guard = Some(Arc::new(generated_guard));
            self
        }

        fn with_run_operation(
            mut self,
            run_operation: impl Fn(
                &AgentPostEditFeedbackInput,
                &[String],
            )
                -> Result<AgentPostEditFeedbackCheckResult, ProfileError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            self.run_operation = Some(Arc::new(run_operation));
            self
        }

        fn recorded(&self) -> Vec<Vec<String>> {
            self.recorded.lock().unwrap().clone()
        }

        fn operations(&self) -> Vec<Vec<String>> {
            self.operations.lock().unwrap().clone()
        }
    }

    impl AgentPostEditFeedbackPorts for MemoryPorts {
        fn extract_touched_paths(
            &self,
            input: &AgentPostEditFeedbackInput,
        ) -> Result<Vec<String>, ProfileError> {
            match &self.extract_touched_paths {
                Some(extract_touched_paths) => extract_touched_paths(input),
                None => Ok(input.touched_path_candidates.clone()),
            }
        }

        fn record_touched_paths(
            &self,
            _input: &AgentPostEditFeedbackInput,
            paths: &[String],
        ) -> Result<(), ProfileError> {
            self.recorded.lock().unwrap().push(paths.to_vec());
            Ok(())
        }

        fn read_touched_paths(
            &self,
            _input: &AgentPostEditFeedbackInput,
        ) -> Result<Vec<String>, ProfileError> {
            Ok(self.stored_paths.clone())
        }

        fn normalize_path(
            &self,
            input: &AgentPostEditFeedbackInput,
            path: &str,
        ) -> Result<Option<String>, ProfileError> {
            match &self.normalize_path {
                Some(normalize_path) => normalize_path(input, path),
                None => Ok(Some(path.strip_prefix("./").unwrap_or(path).to_owned())),
            }
        }

        fn is_inside_project(
            &self,
            _input: &AgentPostEditFeedbackInput,
            normalized_path: &str,
        ) -> Result<bool, ProfileError> {
            Ok(!normalized_path.starts_with("../") && !normalized_path.starts_with('/'))
        }

        fn file_exists(
            &self,
            _input: &AgentPostEditFeedbackInput,
            normalized_path: &str,
        ) -> Result<bool, ProfileError> {
            Ok(self.files.lock().unwrap().contains_key(normalized_path))
        }

        fn read_text(
            &self,
            _input: &AgentPostEditFeedbackInput,
            normalized_path: &str,
        ) -> Result<Option<String>, ProfileError> {
            Ok(self.files.lock().unwrap().get(normalized_path).cloned())
        }

        fn generated_guard(
            &self,
            input: &AgentPostEditFeedbackInput,
            paths: &[String],
        ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError> {
            match &self.generated_guard {
                Some(generated_guard) => generated_guard(input, paths),
                None => Ok(AgentPostEditFeedbackCheckResult::passed()),
            }
        }

        fn run_operation(
            &self,
            input: &AgentPostEditFeedbackInput,
            existing_paths: &[String],
        ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError> {
            match &self.run_operation {
                Some(run_operation) => run_operation(input, existing_paths),
                None => {
                    self.operations
                        .lock()
                        .unwrap()
                        .push(existing_paths.to_vec());
                    Ok(AgentPostEditFeedbackCheckResult::passed())
                }
            }
        }
    }

    fn post_edit_input(paths: &[&str]) -> AgentPostEditFeedbackInput {
        AgentPostEditFeedbackInput {
            cwd: "/repo".to_owned(),
            session_id: "session-1".to_owned(),
            touched_path_candidates: paths.iter().copied().map(str::to_owned).collect(),
            patch_text: None,
            tool_call_id: None,
        }
    }

    fn files(entries: &[(&str, &str)]) -> Files {
        Arc::new(Mutex::new(
            entries
                .iter()
                .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
                .collect(),
        ))
    }

    #[test]
    fn records_extracted_touched_paths_and_passes_a_clean_existing_file() {
        let input = post_edit_input(&["src/clean.ts"]);
        let ports = MemoryPorts::new(files(&[("src/clean.ts", "const value = 1;\n")]));

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(result, AgentPostEditFeedbackResult::default());
        assert_eq!(ports.recorded(), vec![vec!["src/clean.ts".to_owned()]]);
        assert_eq!(ports.operations(), vec![vec!["src/clean.ts".to_owned()]]);
    }

    #[test]
    fn returns_updated_file_when_operation_mutates_one_existing_file() {
        let input = post_edit_input(&["src/format.ts"]);
        let files = files(&[("src/format.ts", "const value=1;\n")]);
        let operation_files = Arc::clone(&files);
        let ports = MemoryPorts::new(files).with_run_operation(move |_input, paths| {
            operation_files
                .lock()
                .unwrap()
                .insert(paths[0].clone(), "const value = 1;\n".to_owned());
            Ok(AgentPostEditFeedbackCheckResult::passed())
        });

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(
            result,
            AgentPostEditFeedbackResult {
                block_reason: None,
                updated_file: Some(AgentPostEditUpdatedFile {
                    path: "src/format.ts".to_owned(),
                    before: "const value=1;\n".to_owned(),
                    after: "const value = 1;\n".to_owned(),
                }),
            }
        );
    }

    #[test]
    fn falls_back_to_stored_paths_and_ignores_missing_or_outside_paths() {
        let input = post_edit_input(&[]);
        let ports = MemoryPorts::new(files(&[("src/existing.ts", "const kept = true;\n")]))
            .with_stored_paths(["src/existing.ts", "src/missing.ts", "../outside.ts"]);

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(result, AgentPostEditFeedbackResult::default());
        assert_eq!(ports.operations(), vec![vec!["src/existing.ts".to_owned()]]);

        let ignored_only_ports =
            MemoryPorts::new(files(&[("src/existing.ts", "const kept = true;\n")]));
        let ignored_only_result = run_agent_post_edit_feedback(
            &post_edit_input(&["src/missing.ts", "../outside.ts"]),
            &ignored_only_ports,
        )
        .unwrap();

        assert_eq!(ignored_only_result, AgentPostEditFeedbackResult::default());
        assert!(ignored_only_ports.operations().is_empty());
    }

    #[test]
    fn returns_block_reason_when_generated_guard_blocks_existing_paths() {
        let input = post_edit_input(&["src/generated.ts"]);
        let ports = MemoryPorts::new(files(&[("src/generated.ts", "generated\n")]))
            .with_generated_guard(|_input, paths| {
                Ok(AgentPostEditFeedbackCheckResult::blocked(format!(
                    "Generated path blocked: {}",
                    paths.join(", ")
                )))
            });

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(
            result,
            AgentPostEditFeedbackResult::block("Generated path blocked: src/generated.ts")
        );
        assert!(ports.operations().is_empty());
    }

    #[test]
    fn returns_operation_block_reason_and_updated_file_when_operation_mutates_then_fails() {
        let input = post_edit_input(&["src/failing.ts"]);
        let files = files(&[("src/failing.ts", "const value=1;\n")]);
        let operation_files = Arc::clone(&files);
        let ports = MemoryPorts::new(files).with_run_operation(move |_input, paths| {
            operation_files
                .lock()
                .unwrap()
                .insert(paths[0].clone(), "const value = 1;\n".to_owned());
            Ok(AgentPostEditFeedbackCheckResult::blocked("autofix failed"))
        });

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(
            result,
            AgentPostEditFeedbackResult {
                block_reason: Some("autofix failed".to_owned()),
                updated_file: Some(AgentPostEditUpdatedFile {
                    path: "src/failing.ts".to_owned(),
                    before: "const value=1;\n".to_owned(),
                    after: "const value = 1;\n".to_owned(),
                }),
            }
        );
    }

    #[test]
    fn supports_apply_patch_extraction_through_injected_path_extraction() {
        let mut input = post_edit_input(&[]);
        input.patch_text =
            Some("*** Begin Patch\n*** Update File: src/patched.ts\n*** End Patch".to_owned());
        let ports = MemoryPorts::new(files(&[("src/patched.ts", "const value = 1;\n")]))
            .with_extract_touched_paths(|input| {
                Ok(
                    if input
                        .patch_text
                        .as_deref()
                        .is_some_and(|text| text.contains("src/patched.ts"))
                    {
                        vec!["src/patched.ts".to_owned()]
                    } else {
                        Vec::new()
                    },
                )
            });

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(result, AgentPostEditFeedbackResult::default());
        assert_eq!(ports.operations(), vec![vec!["src/patched.ts".to_owned()]]);
    }

    #[test]
    fn supports_multi_edit_candidate_lists() {
        let input = post_edit_input(&["src/one.ts", "src/two.ts"]);
        let ports = MemoryPorts::new(files(&[
            ("src/one.ts", "const one = 1;\n"),
            ("src/two.ts", "const two = 2;\n"),
        ]));

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(result, AgentPostEditFeedbackResult::default());
        assert_eq!(
            ports.operations(),
            vec![vec!["src/one.ts".to_owned(), "src/two.ts".to_owned()]]
        );
    }

    #[test]
    fn normalizes_absolute_paths_before_project_boundary_checks() {
        let input = post_edit_input(&["/repo/src/absolute.ts"]);
        let ports = MemoryPorts::new(files(&[("src/absolute.ts", "const value = 1;\n")]))
            .with_normalize_path(|_input, path| Ok(Some(path.replace("/repo/", ""))));

        let result = run_agent_post_edit_feedback(&input, &ports).unwrap();

        assert_eq!(result, AgentPostEditFeedbackResult::default());
        assert_eq!(ports.operations(), vec![vec!["src/absolute.ts".to_owned()]]);
    }

    #[test]
    fn profile_merges_feedback_after_operation_and_overrides_base_fields() {
        let files = files(&[("src/format.ts", "const value=1;\n")]);
        let operation_files = Arc::clone(&files);
        let ports = Arc::new(
            MemoryPorts::new(files).with_run_operation(move |_input, paths| {
                operation_files
                    .lock()
                    .unwrap()
                    .insert(paths[0].clone(), "const value = 1;\n".to_owned());
                Ok(AgentPostEditFeedbackCheckResult::blocked("autofix failed"))
            }),
        );
        let profile =
            agent_post_edit_feedback_profile(AgentPostEditFeedbackProfileOptions::new(ports));
        let output = json!({
            "blockReason": "base failed",
            "updatedFile": {
                "path": "src/base.ts",
                "before": "before",
                "after": "after"
            }
        });
        let input = serde_json::to_value(post_edit_input(&["src/format.ts"])).unwrap();
        let mut context = Value::Null;

        let result = (profile.after_operation.unwrap())(output, &mut context, &input).unwrap();

        assert_eq!(
            result,
            json!({
                "blockReason": "autofix failed",
                "updatedFile": {
                    "path": "src/format.ts",
                    "before": "const value=1;\n",
                    "after": "const value = 1;\n"
                }
            })
        );
    }

    #[test]
    fn profile_merges_feedback_with_operation_error_reason() {
        let ports = Arc::new(MemoryPorts::new(files(&[(
            "src/clean.ts",
            "const value = 1;\n",
        )])));
        let profile =
            agent_post_edit_feedback_profile(AgentPostEditFeedbackProfileOptions::new(ports));
        let input = serde_json::to_value(post_edit_input(&["src/clean.ts"])).unwrap();
        let mut context = Value::Null;

        let result =
            (profile.on_operation_error.unwrap())("operation failed", &mut context, &input)
                .unwrap();

        assert_eq!(result, json!({ "blockReason": "operation failed" }));
    }
}
