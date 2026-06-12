//! Routing from surface triggers to named operations.
//!
//! A [`Binding`] pairs a [`SurfaceTrigger`] (surface name, trigger name,
//! optional phase) with the name of an operation and an ordered list of
//! [`Profile`] middleware. Bindings are composed with the [`bind`] builder
//! and stored in order in a [`BindingRegistry`].
//!
//! At runtime, [`resolve_binding`] matches an incoming [`SurfaceEvent`]
//! against the registry (exact surface/name/phase match) and
//! [`run_bound_operation`] executes the result: profile `before_operation`
//! hooks run in declaration order, the operation runs, then
//! `after_operation` hooks unwind in reverse order. If the operation fails,
//! profile `on_operation_error` hooks may recover by returning a fallback
//! output.
//!
//! [`validate_binding_registry`] reports duplicate triggers and bindings
//! that reference missing operations before anything executes.

use std::collections::HashSet;

use serde_json::Value;

use crate::profiles::{Profile, ProfileError};
use crate::surfaces::surface::{SurfaceEvent, SurfaceTrigger};

/// Ordered list of bindings; earlier entries win on trigger resolution.
pub type BindingRegistry = Vec<Binding>;
/// Operation lookup-and-run callback used by [`run_resolved_binding`]:
/// receives the operation name, its JSON input, and the mutable binding
/// context shared with profile hooks.
pub type BoundOperationFn =
    dyn Fn(&str, Value, &mut Value) -> Result<Value, BindingRunError> + Send + Sync;

/// Routes one surface trigger to a named operation, wrapped in profiles.
#[derive(Debug, Clone)]
pub struct Binding {
    pub trigger: SurfaceTrigger,
    pub operation_name: String,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone)]
pub enum BindingResolution {
    Matched { binding: Binding },
    NotFound { trigger: SurfaceTrigger },
}

#[derive(Debug, Clone)]
pub enum BindingValidationIssue {
    DuplicateTrigger {
        trigger: SurfaceTrigger,
        first_index: usize,
        duplicate_index: usize,
        operation_name: String,
        binding: Binding,
    },
    MissingOperation {
        trigger: SurfaceTrigger,
        binding_index: usize,
        operation_name: String,
        binding: Binding,
    },
}

#[derive(Debug, Clone)]
pub struct BindingValidationResult {
    pub ok: bool,
    pub valid: bool,
    pub issues: Vec<BindingValidationIssue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BoundOperationRunResult {
    Executed { output: Value },
    NotFound { trigger: SurfaceTrigger },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindingRunError {
    #[error("{message}")]
    Operation { message: String },
    #[error("Profile \"{profile}\" failed: {message}")]
    Profile { profile: String, message: String },
}

impl BindingRunError {
    pub fn operation(message: impl Into<String>) -> Self {
        Self::Operation {
            message: message.into(),
        }
    }
}

/// Starts a binding: `bind(trigger).to("operation").finish()` or
/// `.with_profiles(...)`.
pub fn bind(trigger: SurfaceTrigger) -> BindingBuilder {
    BindingBuilder { trigger }
}

pub struct BindingBuilder {
    trigger: SurfaceTrigger,
}

impl BindingBuilder {
    pub fn to(self, operation_name: impl Into<String>) -> BindingProfileBuilder {
        BindingProfileBuilder {
            trigger: self.trigger,
            operation_name: operation_name.into(),
        }
    }
}

pub struct BindingProfileBuilder {
    trigger: SurfaceTrigger,
    operation_name: String,
}

impl BindingProfileBuilder {
    pub fn r#use(self, profiles: impl IntoIterator<Item = Profile>) -> Binding {
        self.with_profiles(profiles)
    }

    pub fn with_profiles(self, profiles: impl IntoIterator<Item = Profile>) -> Binding {
        Binding {
            trigger: self.trigger,
            operation_name: self.operation_name,
            profiles: profiles.into_iter().collect(),
        }
    }

    pub fn finish(self) -> Binding {
        self.with_profiles(std::iter::empty())
    }
}

/// Reports duplicate triggers and, when `operation_names` is provided,
/// bindings that reference operations missing from that set.
pub fn validate_binding_registry(
    registry: &[Binding],
    operation_names: Option<&[&str]>,
) -> BindingValidationResult {
    let mut issues = Vec::new();
    let available_operations =
        operation_names.map(|names| names.iter().copied().collect::<HashSet<_>>());

    for (index, binding) in registry.iter().enumerate() {
        if let Some((prior_index, _)) = registry
            .iter()
            .take(index)
            .enumerate()
            .find(|(_, prior)| same_trigger(&prior.trigger, &binding.trigger))
        {
            issues.push(BindingValidationIssue::DuplicateTrigger {
                trigger: binding.trigger.clone(),
                first_index: prior_index,
                duplicate_index: index,
                operation_name: binding.operation_name.clone(),
                binding: binding.clone(),
            });
        }

        if available_operations
            .as_ref()
            .is_some_and(|operations| !operations.contains(binding.operation_name.as_str()))
        {
            issues.push(BindingValidationIssue::MissingOperation {
                trigger: binding.trigger.clone(),
                binding_index: index,
                operation_name: binding.operation_name.clone(),
                binding: binding.clone(),
            });
        }
    }

    BindingValidationResult {
        ok: issues.is_empty(),
        valid: issues.is_empty(),
        issues,
    }
}

/// Matches an event's trigger against the registry; first exact
/// surface/name/phase match wins.
pub fn resolve_binding(registry: &[Binding], event: &SurfaceEvent) -> BindingResolution {
    resolve_binding_trigger(registry, &event.trigger)
}

pub fn resolve_binding_trigger(
    registry: &[Binding],
    trigger: &SurfaceTrigger,
) -> BindingResolution {
    registry
        .iter()
        .find(|candidate| same_trigger(&candidate.trigger, trigger))
        .cloned()
        .map(|binding| BindingResolution::Matched { binding })
        .unwrap_or_else(|| BindingResolution::NotFound {
            trigger: trigger.clone(),
        })
}

/// Executes a binding's operation inside its profile chain: `before_operation`
/// hooks in order, the operation, then `after_operation` hooks in reverse.
/// On operation failure, the first profile whose `on_operation_error` returns
/// `Ok` supplies the output; otherwise the error propagates.
pub fn run_bound_operation(
    binding: &Binding,
    operation: &(impl Fn(&str, Value, &mut Value) -> Result<Value, BindingRunError> + ?Sized),
    input: Value,
    context: &mut Value,
) -> Result<Value, BindingRunError> {
    let mut next_input = input;

    for profile in &binding.profiles {
        if let Some(before_operation) = &profile.before_operation {
            next_input = before_operation(next_input, context)
                .map_err(|error| profile_error(profile, error))?;
        }
    }

    let mut output = match operation(&binding.operation_name, next_input.clone(), context) {
        Ok(output) => output,
        Err(error) => {
            for profile in &binding.profiles {
                let Some(on_operation_error) = &profile.on_operation_error else {
                    continue;
                };
                if let Ok(output) = on_operation_error(&error.to_string(), context, &next_input) {
                    return Ok(output);
                }
            }
            return Err(error);
        }
    };

    for profile in binding.profiles.iter().rev() {
        if let Some(after_operation) = &profile.after_operation {
            output = after_operation(output, context, &next_input)
                .map_err(|error| profile_error(profile, error))?;
        }
    }

    Ok(output)
}

pub fn run_resolved_binding(
    resolution: &BindingResolution,
    operation: &(impl Fn(&str, Value, &mut Value) -> Result<Value, BindingRunError> + ?Sized),
    input: Value,
    context: &mut Value,
) -> Result<BoundOperationRunResult, BindingRunError> {
    match resolution {
        BindingResolution::Matched { binding } => Ok(BoundOperationRunResult::Executed {
            output: run_bound_operation(binding, operation, input, context)?,
        }),
        BindingResolution::NotFound { trigger } => Ok(BoundOperationRunResult::NotFound {
            trigger: trigger.clone(),
        }),
    }
}

fn profile_error(profile: &Profile, error: ProfileError) -> BindingRunError {
    BindingRunError::Profile {
        profile: profile.name.clone(),
        message: error.message,
    }
}

fn same_trigger(left: &SurfaceTrigger, right: &SurfaceTrigger) -> bool {
    left.surface == right.surface && left.name == right.name && left.phase == right.phase
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{Profile, define_profile};
    use crate::surfaces::surface::{SurfaceEvent, SurfaceTrigger};

    fn edit_trigger() -> SurfaceTrigger {
        SurfaceTrigger {
            surface: "agent-hook".to_owned(),
            name: "post-edit".to_owned(),
            phase: Some("after".to_owned()),
        }
    }

    fn stop_trigger() -> SurfaceTrigger {
        SurfaceTrigger {
            surface: "agent-hook".to_owned(),
            name: "stop".to_owned(),
            phase: Some("before".to_owned()),
        }
    }

    fn event(payload: Value) -> SurfaceEvent {
        SurfaceEvent {
            trigger: edit_trigger(),
            payload,
            metadata: None,
        }
    }

    fn append_operation(
        operation_name: &str,
        input: Value,
        _context: &mut Value,
    ) -> Result<Value, BindingRunError> {
        Ok(serde_json::json!({
            "value": format!("{}:{}", input["value"].as_str().unwrap_or(""), operation_name)
        }))
    }

    fn failing_operation(
        _operation_name: &str,
        _input: Value,
        _context: &mut Value,
    ) -> Result<Value, BindingRunError> {
        Err(BindingRunError::operation("operation failed"))
    }

    fn before_prefix(input: Value, context: &mut Value) -> Result<Value, ProfileError> {
        context["calls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("before"));
        Ok(
            serde_json::json!({ "value": format!("before:{}", input["value"].as_str().unwrap_or("")) }),
        )
    }

    fn after_suffix(
        output: Value,
        context: &mut Value,
        input: &Value,
    ) -> Result<Value, ProfileError> {
        context["calls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(format!(
                "after:{}",
                input["value"].as_str().unwrap_or("")
            )));
        Ok(
            serde_json::json!({ "value": format!("{}:after", output["value"].as_str().unwrap_or("")) }),
        )
    }

    fn fallback(error: &str, _context: &mut Value, _input: &Value) -> Result<Value, ProfileError> {
        Ok(serde_json::json!({ "value": format!("fallback:{error}") }))
    }

    #[test]
    fn exact_trigger_match_routes_to_executor_once() {
        let binding = bind(edit_trigger()).to("postEditFeedback").finish();
        let resolution = resolve_binding(
            &[binding],
            &event(serde_json::json!({ "value": "payload" })),
        );
        let mut context = serde_json::json!({});

        let result = run_resolved_binding(
            &resolution,
            &append_operation,
            serde_json::json!({ "value": "payload" }),
            &mut context,
        )
        .unwrap();

        assert!(matches!(resolution, BindingResolution::Matched { .. }));
        assert_eq!(
            result,
            BoundOperationRunResult::Executed {
                output: serde_json::json!({ "value": "payload:postEditFeedback" })
            }
        );
    }

    #[test]
    fn no_exact_trigger_match_returns_not_found_without_executor() {
        let binding = bind(stop_trigger()).to("stopValidation").finish();
        let resolution = resolve_binding(
            &[binding],
            &event(serde_json::json!({ "value": "payload" })),
        );
        let mut context = serde_json::json!({});

        let result = run_resolved_binding(
            &resolution,
            &failing_operation,
            serde_json::json!({ "value": "payload" }),
            &mut context,
        )
        .unwrap();

        assert_eq!(
            result,
            BoundOperationRunResult::NotFound {
                trigger: edit_trigger()
            }
        );
    }

    #[test]
    fn duplicate_and_missing_operation_validation_issues_are_reported() {
        let first = bind(edit_trigger()).to("first").finish();
        let duplicate = bind(edit_trigger()).to("second").finish();
        let validation = validate_binding_registry(&[first, duplicate], Some(&["first"]));

        assert!(!validation.ok);
        assert_eq!(validation.issues.len(), 2);
        assert!(matches!(
            validation.issues[0],
            BindingValidationIssue::DuplicateTrigger {
                first_index: 0,
                duplicate_index: 1,
                ..
            }
        ));
        assert!(matches!(
            validation.issues[1],
            BindingValidationIssue::MissingOperation {
                binding_index: 1,
                ..
            }
        ));
    }

    #[test]
    fn profiles_transform_input_and_unwind_output() {
        let binding = bind(edit_trigger()).to("handled").with_profiles(vec![
            define_profile(Profile::new("input-prefix").before_operation(before_prefix)),
            define_profile(Profile::new("output-suffix").after_operation(after_suffix)),
        ]);
        let mut context = serde_json::json!({ "calls": [] });

        let output = run_bound_operation(
            &binding,
            &append_operation,
            serde_json::json!({ "value": "payload" }),
            &mut context,
        )
        .unwrap();

        assert_eq!(
            output,
            serde_json::json!({ "value": "before:payload:handled:after" })
        );
        assert_eq!(
            context["calls"],
            serde_json::json!(["before", "after:before:payload"])
        );
    }

    #[test]
    fn operation_errors_use_profile_fallback_or_rethrow() {
        let handled = bind(edit_trigger())
            .to("handled")
            .with_profiles(vec![define_profile(
                Profile::new("fallback").on_operation_error(fallback),
            )]);
        let unhandled = bind(edit_trigger()).to("unhandled").finish();
        let mut context = serde_json::json!({});

        let handled_output = run_bound_operation(
            &handled,
            &failing_operation,
            serde_json::json!({ "value": "payload" }),
            &mut context,
        )
        .unwrap();
        let unhandled_error = run_bound_operation(
            &unhandled,
            &failing_operation,
            serde_json::json!({ "value": "payload" }),
            &mut context,
        )
        .unwrap_err();

        assert_eq!(
            handled_output,
            serde_json::json!({ "value": "fallback:operation failed" })
        );
        assert_eq!(
            unhandled_error,
            BindingRunError::operation("operation failed")
        );
    }
}
