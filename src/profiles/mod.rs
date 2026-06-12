//! Middleware around operation execution, plus shipped agent-safety profiles.
//!
//! A [`Profile`] is a named set of optional hooks that a
//! [`Binding`](crate::bindings::Binding) wraps around its operation:
//! `before_operation` transforms the input, `after_operation` transforms the
//! output (applied in reverse declaration order), and `on_operation_error`
//! may recover from a failed operation by returning a fallback output. All
//! hooks share a mutable JSON context and fail with [`ProfileError`].
//!
//! Three profiles ship with the crate:
//!
//! - [`stop_session_validation`] — gates agent session stops: fingerprints
//!   repo state, runs project validation against touched paths, and blocks
//!   the stop with a reason when validation fails.
//! - [`agent_post_edit_feedback`] — reacts to agent file edits: filters
//!   touched paths to in-project files, runs feedback checks, and can return
//!   updated file content.
//! - [`generated_file_guard`](generated_file_guard()) — blocks edits to generated or protected files
//!   matched by exact path, prefix, pattern, or predicate rules.
//!
//! Each shipped profile takes its host capabilities as a `*Ports` trait so
//! callers control filesystem, Git, and session-state access.

use std::sync::Arc;

use serde_json::Value;

pub mod agent_post_edit_feedback;
pub mod generated_file_guard;
pub mod stop_session_validation;

pub use agent_post_edit_feedback::{
    AgentPostEditFeedbackCheckResult, AgentPostEditFeedbackInput, AgentPostEditFeedbackPorts,
    AgentPostEditFeedbackProfileOptions, AgentPostEditFeedbackResult, AgentPostEditUpdatedFile,
    agent_post_edit_feedback_profile, run_agent_post_edit_feedback,
};
pub use generated_file_guard::{
    GeneratedFileGuard, GeneratedFileGuardFileRule, GeneratedFileGuardOptions,
    GeneratedFileGuardPatternRule, GeneratedFileGuardPredicate, GeneratedFileGuardPredicateRule,
    GeneratedFileGuardPrefixRule, GeneratedFileGuardResult, generated_file_guard,
};
pub use stop_session_validation::{
    StopSessionFingerprint, StopSessionFingerprintResult, StopSessionGeneratedGuardInput,
    StopSessionGeneratedGuardResult, StopSessionValidationBlockedError, StopSessionValidationEnv,
    StopSessionValidationEnvFn, StopSessionValidationEnvInput, StopSessionValidationInput,
    StopSessionValidationOptions, StopSessionValidationPorts, StopSessionValidationResult,
    StopSessionValidationRunInput, StopSessionValidationRunResult,
    create_stop_session_validation_profile, run_stop_session_validation,
};

pub type BeforeOperationFn =
    Arc<dyn Fn(Value, &mut Value) -> Result<Value, ProfileError> + Send + Sync + 'static>;
pub type AfterOperationFn =
    Arc<dyn Fn(Value, &mut Value, &Value) -> Result<Value, ProfileError> + Send + Sync + 'static>;
pub type OnOperationErrorFn =
    Arc<dyn Fn(&str, &mut Value, &Value) -> Result<Value, ProfileError> + Send + Sync + 'static>;

/// Failure raised by a profile hook; surfaces as
/// [`BindingRunError::Profile`](crate::bindings::BindingRunError::Profile)
/// with the profile's name attached.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProfileError {
    pub message: String,
}

impl ProfileError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Named operation middleware: optional before/after/on-error hooks sharing
/// a mutable JSON context. Built fluently from [`Profile::new`].
#[derive(Clone)]
pub struct Profile {
    pub name: String,
    pub before_operation: Option<BeforeOperationFn>,
    pub after_operation: Option<AfterOperationFn>,
    pub on_operation_error: Option<OnOperationErrorFn>,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            before_operation: None,
            after_operation: None,
            on_operation_error: None,
        }
    }

    pub fn before_operation(
        mut self,
        hook: impl Fn(Value, &mut Value) -> Result<Value, ProfileError> + Send + Sync + 'static,
    ) -> Self {
        self.before_operation = Some(Arc::new(hook));
        self
    }

    pub fn after_operation(
        mut self,
        hook: impl Fn(Value, &mut Value, &Value) -> Result<Value, ProfileError> + Send + Sync + 'static,
    ) -> Self {
        self.after_operation = Some(Arc::new(hook));
        self
    }

    pub fn on_operation_error(
        mut self,
        hook: impl Fn(&str, &mut Value, &Value) -> Result<Value, ProfileError> + Send + Sync + 'static,
    ) -> Self {
        self.on_operation_error = Some(Arc::new(hook));
        self
    }
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Profile")
            .field("name", &self.name)
            .field(
                "before_operation",
                &self.before_operation.as_ref().map(|_| "<fn>"),
            )
            .field(
                "after_operation",
                &self.after_operation.as_ref().map(|_| "<fn>"),
            )
            .field(
                "on_operation_error",
                &self.on_operation_error.as_ref().map(|_| "<fn>"),
            )
            .finish()
    }
}

/// Identity function marking a value as a complete profile definition.
pub fn define_profile(profile: Profile) -> Profile {
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_profile_preserves_name_and_hooks() {
        let profile =
            define_profile(Profile::new("example").before_operation(|input, _context| Ok(input)));

        assert_eq!(profile.name, "example");
        assert!(profile.before_operation.is_some());
    }
}
