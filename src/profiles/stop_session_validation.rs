use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Profile, ProfileError, define_profile};

pub type StopSessionValidationEnv = BTreeMap<String, Option<String>>;
pub type StopSessionValidationEnvFn = Arc<
    dyn for<'a> Fn(
            &StopSessionValidationEnvInput<'a>,
        ) -> Result<StopSessionValidationEnv, ProfileError>
        + Send
        + Sync
        + 'static,
>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSessionValidationInput {
    pub cwd: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,
    #[serde(default)]
    pub touched_path_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopSessionFingerprint {
    pub signature: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopSessionFingerprintResult {
    Captured { fingerprint: StopSessionFingerprint },
    Failed { reason: String },
}

impl StopSessionFingerprintResult {
    pub fn captured(fingerprint: StopSessionFingerprint) -> Self {
        Self::Captured { fingerprint }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopSessionValidationRunResult {
    Accepted {
        system_message: Option<String>,
    },
    Blocked {
        block_reason: String,
        system_message: Option<String>,
    },
}

impl StopSessionValidationRunResult {
    pub fn accepted() -> Self {
        Self::Accepted {
            system_message: None,
        }
    }

    pub fn accepted_with_message(message: impl Into<String>) -> Self {
        Self::Accepted {
            system_message: Some(message.into()),
        }
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            block_reason: reason.into(),
            system_message: None,
        }
    }

    pub fn blocked_with_message(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Blocked {
            block_reason: reason.into(),
            system_message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSessionValidationResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
}

impl StopSessionValidationResult {
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            block_reason: Some(reason.into()),
            system_message: None,
        }
    }

    pub fn block_with_message(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            block_reason: Some(reason.into()),
            system_message: Some(message.into()),
        }
    }

    pub fn pass_with_message(message: impl Into<String>) -> Self {
        Self {
            block_reason: None,
            system_message: Some(message.into()),
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.block_reason.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopSessionGeneratedGuardInput<'a> {
    pub input: &'a StopSessionValidationInput,
    pub root: &'a str,
    pub touched_paths: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopSessionGeneratedGuardResult {
    Allowed,
    Blocked {
        block_reason: String,
        system_message: Option<String>,
    },
}

impl StopSessionGeneratedGuardResult {
    pub fn allowed() -> Self {
        Self::Allowed
    }

    pub fn blocked(reason: impl Into<String>) -> Self {
        Self::Blocked {
            block_reason: reason.into(),
            system_message: None,
        }
    }

    pub fn blocked_with_message(reason: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Blocked {
            block_reason: reason.into(),
            system_message: Some(message.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopSessionValidationEnvInput<'a> {
    pub input: &'a StopSessionValidationInput,
    pub root: &'a str,
    pub run_id: &'a str,
    pub touched_paths: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopSessionValidationRunInput<'a> {
    pub input: &'a StopSessionValidationInput,
    pub root: &'a str,
    pub run_id: &'a str,
    pub touched_paths: &'a [String],
    pub env: StopSessionValidationEnv,
}

pub trait StopSessionValidationPorts: Send + Sync {
    fn root(&self, cwd: &str) -> Result<String, ProfileError>;

    fn extract_touched_paths(
        &self,
        input: &StopSessionValidationInput,
        root: &str,
    ) -> Result<Vec<String>, ProfileError>;

    fn read_touched_paths(
        &self,
        input: &StopSessionValidationInput,
    ) -> Result<Vec<String>, ProfileError>;

    fn clear_touched_paths(&self, input: &StopSessionValidationInput) -> Result<(), ProfileError>;

    fn generated_guard(
        &self,
        input: StopSessionGeneratedGuardInput<'_>,
    ) -> Result<StopSessionGeneratedGuardResult, ProfileError>;

    fn capture_fingerprint(&self, root: &str)
    -> Result<StopSessionFingerprintResult, ProfileError>;

    fn run_validation(
        &self,
        input: StopSessionValidationRunInput<'_>,
    ) -> Result<StopSessionValidationRunResult, ProfileError>;

    fn create_id(&self) -> String;
}

#[derive(Clone)]
pub struct StopSessionValidationOptions {
    pub ports: Arc<dyn StopSessionValidationPorts>,
    pub build_validation_env: Option<StopSessionValidationEnvFn>,
}

impl StopSessionValidationOptions {
    pub fn new(ports: Arc<dyn StopSessionValidationPorts>) -> Self {
        Self {
            ports,
            build_validation_env: None,
        }
    }

    pub fn with_validation_env(
        mut self,
        build_validation_env: impl for<'a> Fn(
            &StopSessionValidationEnvInput<'a>,
        ) -> Result<StopSessionValidationEnv, ProfileError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.build_validation_env = Some(Arc::new(build_validation_env));
        self
    }
}

impl std::fmt::Debug for StopSessionValidationOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StopSessionValidationOptions")
            .field("ports", &"<ports>")
            .field(
                "build_validation_env",
                &self.build_validation_env.as_ref().map(|_| "<fn>"),
            )
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct StopSessionValidationBlockedError {
    pub result: StopSessionValidationResult,
    message: String,
}

impl StopSessionValidationBlockedError {
    pub fn new(result: StopSessionValidationResult) -> Self {
        let message = result
            .block_reason
            .clone()
            .unwrap_or_else(|| "Stop-session validation blocked.".to_owned());
        Self { result, message }
    }
}

pub fn run_stop_session_validation(
    input: &StopSessionValidationInput,
    options: &StopSessionValidationOptions,
) -> Result<StopSessionValidationResult, ProfileError> {
    if input.stop_hook_active == Some(true) {
        return Ok(StopSessionValidationResult::default());
    }

    let root = options.ports.root(&input.cwd)?;
    let touched_paths = merge_touched_paths(
        &options.ports.extract_touched_paths(input, &root)?,
        &options.ports.read_touched_paths(input)?,
    );
    if touched_paths.is_empty() {
        return Ok(StopSessionValidationResult::default());
    }

    match options
        .ports
        .generated_guard(StopSessionGeneratedGuardInput {
            input,
            root: &root,
            touched_paths: &touched_paths,
        })? {
        StopSessionGeneratedGuardResult::Allowed => {}
        StopSessionGeneratedGuardResult::Blocked {
            block_reason,
            system_message,
        } => return Ok(block(block_reason, system_message)),
    }

    let run_id = options.ports.create_id();
    let before = match options.ports.capture_fingerprint(&root)? {
        StopSessionFingerprintResult::Captured { fingerprint } => fingerprint,
        StopSessionFingerprintResult::Failed { reason } => {
            return Ok(StopSessionValidationResult::block(
                read_only_proof_unavailable_message(&run_id, &reason),
            ));
        }
    };

    let env_input = StopSessionValidationEnvInput {
        input,
        root: &root,
        run_id: &run_id,
        touched_paths: &touched_paths,
    };
    let env = match &options.build_validation_env {
        Some(build_validation_env) => build_validation_env(&env_input)?,
        None => StopSessionValidationEnv::new(),
    };
    let validation = options
        .ports
        .run_validation(StopSessionValidationRunInput {
            input,
            root: &root,
            run_id: &run_id,
            touched_paths: &touched_paths,
            env,
        })?;

    let after = match options.ports.capture_fingerprint(&root)? {
        StopSessionFingerprintResult::Captured { fingerprint } => fingerprint,
        StopSessionFingerprintResult::Failed { reason } => {
            return Ok(StopSessionValidationResult::block(
                read_only_proof_unavailable_message(&run_id, &reason),
            ));
        }
    };

    if before.signature != after.signature {
        return Ok(StopSessionValidationResult::block(
            read_only_violation_message(&run_id, &changed_fingerprint_paths(&before, &after)),
        ));
    }

    match validation {
        StopSessionValidationRunResult::Accepted { system_message } => {
            options.ports.clear_touched_paths(input)?;
            Ok(match system_message {
                Some(system_message) => {
                    StopSessionValidationResult::pass_with_message(system_message)
                }
                None => StopSessionValidationResult::default(),
            })
        }
        StopSessionValidationRunResult::Blocked {
            block_reason,
            system_message,
        } => Ok(block(block_reason, system_message)),
    }
}

/// Builds the stop-session validation [`Profile`]: on a stop event it
/// extracts touched paths, runs the generated-file guard, executes the
/// project validation run through the configured ports, and blocks the stop
/// with a reason when validation fails.
pub fn create_stop_session_validation_profile(options: StopSessionValidationOptions) -> Profile {
    let options = Arc::new(options);
    define_profile(Profile::new("stop-session-validation").before_operation(
        move |input, _context| {
            let validation_input = stop_session_validation_input_from_value(&input)?;
            let result = run_stop_session_validation(&validation_input, options.as_ref())?;
            if result.is_blocked() {
                let error = StopSessionValidationBlockedError::new(result);
                return Err(ProfileError::new(error.to_string()));
            }
            Ok(input)
        },
    ))
}

fn merge_touched_paths(extracted_paths: &[String], stored_paths: &[String]) -> Vec<String> {
    if extracted_paths.is_empty() {
        return stored_paths.to_vec();
    }
    if stored_paths.is_empty() || extracted_paths == stored_paths {
        return extracted_paths.to_vec();
    }

    extracted_paths
        .iter()
        .chain(stored_paths)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn changed_fingerprint_paths(
    before: &StopSessionFingerprint,
    after: &StopSessionFingerprint,
) -> Vec<String> {
    before
        .paths
        .iter()
        .chain(&after.paths)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn read_only_proof_unavailable_message(run_id: &str, reason: &str) -> String {
    [
        format!("Stop validation read-only proof unavailable ({run_id})."),
        reason.to_owned(),
        "Stop validation is blocked because it cannot prove the run preserved repository state."
            .to_owned(),
    ]
    .join("\n")
}

fn read_only_violation_message(run_id: &str, changed_paths: &[String]) -> String {
    let paths = if changed_paths.is_empty() {
        "unknown repository path".to_owned()
    } else {
        changed_paths.join(", ")
    };
    [
        format!("Stop validation read-only violation ({run_id})."),
        format!("Validation mutated repository state: {paths}"),
        "Run the needed fix or sync outside the Stop hook, then retry.".to_owned(),
    ]
    .join("\n")
}

fn block(
    block_reason: impl Into<String>,
    system_message: Option<String>,
) -> StopSessionValidationResult {
    StopSessionValidationResult {
        block_reason: Some(block_reason.into()),
        system_message,
    }
}

fn stop_session_validation_input_from_value(
    value: &Value,
) -> Result<StopSessionValidationInput, ProfileError> {
    serde_json::from_value(value.clone()).map_err(|error| {
        ProfileError::new(format!("Invalid stop-session validation input: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    type ValidationFn = dyn for<'a> Fn(
            StopSessionValidationRunInput<'a>,
        ) -> Result<StopSessionValidationRunResult, ProfileError>
        + Send
        + Sync;

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Calls {
        root: usize,
        extract: usize,
        read: usize,
        clear: usize,
        generated: usize,
        fingerprint: usize,
        validation: usize,
    }

    struct TestPorts {
        extracted: Vec<String>,
        stored: Vec<String>,
        generated_block_reason: Option<String>,
        fingerprints: Mutex<Vec<StopSessionFingerprintResult>>,
        validation: Option<Arc<ValidationFn>>,
        calls: Mutex<Calls>,
        cleared: Mutex<bool>,
        validation_inputs: Mutex<Vec<CapturedValidationInput>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CapturedValidationInput {
        root: String,
        run_id: String,
        touched_paths: Vec<String>,
        env: StopSessionValidationEnv,
    }

    impl TestPorts {
        fn new() -> Self {
            let clean_fingerprint =
                StopSessionFingerprintResult::captured(StopSessionFingerprint {
                    signature: "clean".to_owned(),
                    paths: Vec::new(),
                });
            Self {
                extracted: vec!["src/index.ts".to_owned()],
                stored: Vec::new(),
                generated_block_reason: None,
                fingerprints: Mutex::new(vec![clean_fingerprint.clone(), clean_fingerprint]),
                validation: None,
                calls: Mutex::new(Calls::default()),
                cleared: Mutex::new(false),
                validation_inputs: Mutex::new(Vec::new()),
            }
        }

        fn with_extracted(mut self, paths: &[&str]) -> Self {
            self.extracted = paths.iter().copied().map(str::to_owned).collect();
            self
        }

        fn with_stored(mut self, paths: &[&str]) -> Self {
            self.stored = paths.iter().copied().map(str::to_owned).collect();
            self
        }

        fn with_generated_block_reason(mut self, reason: impl Into<String>) -> Self {
            self.generated_block_reason = Some(reason.into());
            self
        }

        fn with_fingerprints(mut self, fingerprints: Vec<StopSessionFingerprintResult>) -> Self {
            self.fingerprints = Mutex::new(fingerprints);
            self
        }

        fn with_validation(
            mut self,
            validation: impl for<'a> Fn(
                StopSessionValidationRunInput<'a>,
            )
                -> Result<StopSessionValidationRunResult, ProfileError>
            + Send
            + Sync
            + 'static,
        ) -> Self {
            self.validation = Some(Arc::new(validation));
            self
        }

        fn calls(&self) -> Calls {
            let calls = self.calls.lock().unwrap();
            Calls {
                root: calls.root,
                extract: calls.extract,
                read: calls.read,
                clear: calls.clear,
                generated: calls.generated,
                fingerprint: calls.fingerprint,
                validation: calls.validation,
            }
        }

        fn cleared(&self) -> bool {
            *self.cleared.lock().unwrap()
        }

        fn validation_inputs(&self) -> Vec<CapturedValidationInput> {
            self.validation_inputs.lock().unwrap().clone()
        }
    }

    impl StopSessionValidationPorts for TestPorts {
        fn root(&self, cwd: &str) -> Result<String, ProfileError> {
            self.calls.lock().unwrap().root += 1;
            Ok(cwd.to_owned())
        }

        fn extract_touched_paths(
            &self,
            _input: &StopSessionValidationInput,
            _root: &str,
        ) -> Result<Vec<String>, ProfileError> {
            self.calls.lock().unwrap().extract += 1;
            Ok(self.extracted.clone())
        }

        fn read_touched_paths(
            &self,
            _input: &StopSessionValidationInput,
        ) -> Result<Vec<String>, ProfileError> {
            self.calls.lock().unwrap().read += 1;
            Ok(self.stored.clone())
        }

        fn clear_touched_paths(
            &self,
            _input: &StopSessionValidationInput,
        ) -> Result<(), ProfileError> {
            self.calls.lock().unwrap().clear += 1;
            *self.cleared.lock().unwrap() = true;
            Ok(())
        }

        fn generated_guard(
            &self,
            _input: StopSessionGeneratedGuardInput<'_>,
        ) -> Result<StopSessionGeneratedGuardResult, ProfileError> {
            self.calls.lock().unwrap().generated += 1;
            Ok(match &self.generated_block_reason {
                Some(reason) => StopSessionGeneratedGuardResult::blocked(reason.clone()),
                None => StopSessionGeneratedGuardResult::allowed(),
            })
        }

        fn capture_fingerprint(
            &self,
            _root: &str,
        ) -> Result<StopSessionFingerprintResult, ProfileError> {
            self.calls.lock().unwrap().fingerprint += 1;
            let mut fingerprints = self.fingerprints.lock().unwrap();
            if fingerprints.is_empty() {
                return Ok(StopSessionFingerprintResult::captured(
                    StopSessionFingerprint {
                        signature: "clean".to_owned(),
                        paths: Vec::new(),
                    },
                ));
            }
            Ok(fingerprints.remove(0))
        }

        fn run_validation(
            &self,
            input: StopSessionValidationRunInput<'_>,
        ) -> Result<StopSessionValidationRunResult, ProfileError> {
            self.calls.lock().unwrap().validation += 1;
            self.validation_inputs
                .lock()
                .unwrap()
                .push(CapturedValidationInput {
                    root: input.root.to_owned(),
                    run_id: input.run_id.to_owned(),
                    touched_paths: input.touched_paths.to_vec(),
                    env: input.env.clone(),
                });
            match &self.validation {
                Some(validation) => validation(input),
                None => Ok(StopSessionValidationRunResult::accepted()),
            }
        }

        fn create_id(&self) -> String {
            "run-1".to_owned()
        }
    }

    fn input() -> StopSessionValidationInput {
        StopSessionValidationInput {
            cwd: "/repo/worktree".to_owned(),
            session_id: "session-1".to_owned(),
            stop_hook_active: None,
            touched_path_candidates: vec!["src/index.ts".to_owned()],
        }
    }

    fn options(ports: Arc<dyn StopSessionValidationPorts>) -> StopSessionValidationOptions {
        StopSessionValidationOptions::new(ports).with_validation_env(|input| {
            Ok(BTreeMap::from([
                ("RUN_ID".to_owned(), Some(input.run_id.to_owned())),
                (
                    "SESSION_ID".to_owned(),
                    Some(input.input.session_id.clone()),
                ),
                (
                    "TOUCHED_PATHS".to_owned(),
                    Some(input.touched_paths.join("\n")),
                ),
            ]))
        })
    }

    fn captured(signature: &str, paths: &[&str]) -> StopSessionFingerprintResult {
        StopSessionFingerprintResult::captured(StopSessionFingerprint {
            signature: signature.to_owned(),
            paths: paths.iter().copied().map(str::to_owned).collect(),
        })
    }

    #[test]
    fn skips_validation_and_state_cleanup_when_no_touched_paths_are_known() {
        let ports = Arc::new(TestPorts::new().with_extracted(&[]).with_stored(&[]));
        let mut input = input();
        input.touched_path_candidates = Vec::new();

        let result = run_stop_session_validation(&input, &options(ports.clone())).unwrap();

        assert_eq!(result.block_reason, None);
        assert_eq!(
            ports.calls(),
            Calls {
                root: 1,
                extract: 1,
                read: 1,
                clear: 0,
                generated: 0,
                fingerprint: 0,
                validation: 0,
            }
        );
        assert!(!ports.cleared());
    }

    #[test]
    fn skips_validation_when_the_stop_hook_is_already_active() {
        let ports = Arc::new(TestPorts::new());
        let mut input = input();
        input.stop_hook_active = Some(true);

        let result = run_stop_session_validation(&input, &options(ports.clone())).unwrap();

        assert_eq!(result.block_reason, None);
        assert_eq!(ports.calls(), Calls::default());
    }

    #[test]
    fn blocks_generated_or_protected_touched_paths_before_fingerprinting() {
        let ports = Arc::new(
            TestPorts::new()
                .with_generated_block_reason("Generated files must not be edited directly"),
        );

        let result = run_stop_session_validation(&input(), &options(ports.clone())).unwrap();

        assert_eq!(
            result.block_reason.as_deref(),
            Some("Generated files must not be edited directly")
        );
        let calls = ports.calls();
        assert_eq!(calls.generated, 1);
        assert_eq!(calls.fingerprint, 0);
        assert_eq!(calls.validation, 0);
        assert_eq!(calls.clear, 0);
    }

    #[test]
    fn blocks_validation_failures_and_preserves_session_state() {
        let ports = Arc::new(
            TestPorts::new()
                .with_stored(&["src/other.ts"])
                .with_validation(|_input| {
                    Ok(StopSessionValidationRunResult::blocked(
                        "Stop validation failed",
                    ))
                }),
        );

        let result = run_stop_session_validation(&input(), &options(ports.clone())).unwrap();

        assert_eq!(
            result.block_reason.as_deref(),
            Some("Stop validation failed")
        );
        let calls = ports.calls();
        assert_eq!(calls.validation, 1);
        assert_eq!(calls.clear, 0);
        assert_eq!(
            ports.validation_inputs()[0].touched_paths,
            vec!["src/index.ts".to_owned(), "src/other.ts".to_owned()]
        );
        assert_eq!(
            ports.validation_inputs()[0].env,
            BTreeMap::from([
                ("RUN_ID".to_owned(), Some("run-1".to_owned())),
                ("SESSION_ID".to_owned(), Some("session-1".to_owned())),
                (
                    "TOUCHED_PATHS".to_owned(),
                    Some("src/index.ts\nsrc/other.ts".to_owned()),
                ),
            ])
        );
    }

    #[test]
    fn blocks_when_validation_mutates_fingerprinted_repository_state() {
        let ports = Arc::new(TestPorts::new().with_fingerprints(vec![
            captured("before", &["src/index.ts"]),
            captured("after", &["src/index.ts", "src/new.ts"]),
        ]));

        let result = run_stop_session_validation(&input(), &options(ports.clone())).unwrap();
        let reason = result.block_reason.unwrap();

        assert!(reason.contains("Stop validation read-only violation (run-1)"));
        assert!(reason.contains("src/index.ts, src/new.ts"));
        let calls = ports.calls();
        assert_eq!(calls.validation, 1);
        assert_eq!(calls.clear, 0);
    }

    #[test]
    fn blocks_when_read_only_proof_is_unavailable() {
        let ports = Arc::new(TestPorts::new().with_fingerprints(vec![
            StopSessionFingerprintResult::failed("git status failed"),
        ]));

        let result = run_stop_session_validation(&input(), &options(ports.clone())).unwrap();
        let reason = result.block_reason.unwrap();

        assert!(reason.contains("Stop validation read-only proof unavailable (run-1)."));
        assert!(reason.contains("git status failed"));
        assert_eq!(ports.calls().validation, 0);
    }

    #[test]
    fn clears_session_state_after_successful_validation() {
        let ports = Arc::new(TestPorts::new().with_stored(&["src/index.ts"]));

        let result = run_stop_session_validation(&input(), &options(ports.clone())).unwrap();

        assert_eq!(result.block_reason, None);
        let calls = ports.calls();
        assert_eq!(calls.fingerprint, 2);
        assert_eq!(calls.validation, 1);
        assert_eq!(calls.clear, 1);
        assert!(ports.cleared());
    }

    #[test]
    fn returns_system_message_after_successful_validation() {
        let ports = Arc::new(TestPorts::new().with_validation(|_input| {
            Ok(StopSessionValidationRunResult::accepted_with_message(
                "validation passed",
            ))
        }));

        let result = run_stop_session_validation(&input(), &options(ports)).unwrap();

        assert_eq!(
            result,
            StopSessionValidationResult::pass_with_message("validation passed")
        );
    }

    #[test]
    fn profile_blocks_before_operation_with_validation_error_reason() {
        let ports = Arc::new(TestPorts::new().with_validation(|_input| {
            Ok(StopSessionValidationRunResult::blocked(
                "Stop validation failed",
            ))
        }));
        let profile = create_stop_session_validation_profile(options(ports));
        let input = serde_json::to_value(input()).unwrap();
        let mut context = Value::Null;

        let error = (profile.before_operation.unwrap())(input, &mut context).unwrap_err();

        assert_eq!(error.message, "Stop validation failed");
    }

    #[test]
    fn profile_returns_input_after_successful_validation() {
        let ports = Arc::new(TestPorts::new());
        let profile = create_stop_session_validation_profile(options(ports));
        let input = serde_json::to_value(input()).unwrap();
        let mut context = Value::Null;

        let result = (profile.before_operation.unwrap())(input.clone(), &mut context).unwrap();

        assert_eq!(result, input);
    }
}
