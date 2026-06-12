use std::collections::BTreeMap;

use super::harness_hook_config::{
    HarnessHookConfig, HarnessHookConfigRegistry, HarnessOptions, HarnessTarget, HookBinding,
};
use super::runtime::HarnessCodec;

pub type HarnessRegistry<'a> = BTreeMap<String, &'a dyn HarnessCodec>;

/// One agent harness: its protocol codec plus the recipe for rendering its
/// native hook configuration file. Built-ins: [`claude_harness`](super::claude_harness)
/// and [`codex_harness`](super::codex_harness); custom harnesses via
/// [`define_harness`].
#[derive(Clone)]
pub struct Harness<'a> {
    pub id: String,
    pub codec: &'a dyn HarnessCodec,
    pub hook_config: HarnessHookConfig,
}

pub struct HarnessDefinition<'a> {
    pub id: String,
    pub codec: &'a dyn HarnessCodec,
    pub hook_config: HarnessHookConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HarnessTargetInput {
    pub path: Option<String>,
    pub command_prefix: String,
    pub options: HarnessOptions,
}

impl HarnessTargetInput {
    pub fn new(command_prefix: impl Into<String>) -> Self {
        Self {
            path: None,
            command_prefix: command_prefix.into(),
            options: HarnessOptions::new(),
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_options(mut self, options: HarnessOptions) -> Self {
        self.options = options;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookBindingInput {
    pub matcher: Option<String>,
    pub command_prefix: Option<String>,
    pub timeout: u32,
    pub status_message: String,
    pub options: HarnessOptions,
}

impl HookBindingInput {
    pub fn new(timeout: u32, status_message: impl Into<String>) -> Self {
        Self {
            matcher: None,
            command_prefix: None,
            timeout,
            status_message: status_message.into(),
            options: HarnessOptions::new(),
        }
    }

    pub fn with_matcher(mut self, matcher: impl Into<String>) -> Self {
        self.matcher = Some(matcher.into());
        self
    }

    pub fn with_command_prefix(mut self, command_prefix: impl Into<String>) -> Self {
        self.command_prefix = Some(command_prefix.into());
        self
    }

    pub fn with_options(mut self, options: HarnessOptions) -> Self {
        self.options = options;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HarnessError {
    #[error("Duplicate harness id: {id}")]
    DuplicateHarnessId { id: String },
}

pub fn define_harness(definition: HarnessDefinition<'_>) -> Harness<'_> {
    Harness {
        id: definition.id,
        codec: definition.codec,
        hook_config: definition.hook_config,
    }
}

impl Harness<'_> {
    pub fn target(&self, input: HarnessTargetInput) -> HarnessTarget {
        HarnessTarget {
            harness: self.id.clone(),
            path: input
                .path
                .unwrap_or_else(|| self.hook_config.default_path.clone()),
            command_prefix: input.command_prefix,
            options: input.options,
        }
    }

    pub fn bind(&self, input: HookBindingInput) -> HookBinding {
        HookBinding {
            harness: self.id.clone(),
            matcher: input.matcher,
            command_prefix: input.command_prefix,
            timeout: input.timeout,
            status_message: input.status_message,
            options: input.options,
        }
    }
}

pub fn harness_registry_from_harnesses<'codec>(
    harnesses: &[Harness<'codec>],
) -> Result<HarnessRegistry<'codec>, HarnessError> {
    validate_unique_harness_ids(harnesses)?;
    Ok(harnesses
        .iter()
        .map(|harness| (harness.id.clone(), harness.codec))
        .collect())
}

pub fn harness_hook_config_registry_from_harnesses(
    harnesses: &[Harness<'_>],
) -> Result<HarnessHookConfigRegistry, HarnessError> {
    validate_unique_harness_ids(harnesses)?;
    Ok(harnesses
        .iter()
        .map(|harness| (harness.id.clone(), harness.hook_config.clone()))
        .collect())
}

fn validate_unique_harness_ids(harnesses: &[Harness<'_>]) -> Result<(), HarnessError> {
    let mut seen = BTreeMap::new();
    for harness in harnesses {
        if seen.insert(harness.id.as_str(), ()).is_some() {
            return Err(HarnessError::DuplicateHarnessId {
                id: harness.id.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::contract::{
        HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage,
    };
    use super::super::harness_hook_config::HarnessHookConfigRenderInput;
    use super::*;
    use anyhow::Result;

    struct FixtureCodec;

    impl HarnessCodec for FixtureCodec {
        fn harness(&self) -> &'static str {
            "fixture"
        }

        fn decode(&self, _stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
            Ok(HookRequest {
                event: HookEvent {
                    harness: "fixture".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/repo".to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: None,
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(&self, _outcome: HookOutcome, _request: &HookRequest) -> HookEmission {
            HookEmission {
                exit_code: 0,
                stdout: None,
                stderr: None,
            }
        }

        fn encode_failure(&self, _stage: HookStage, _error: &anyhow::Error) -> HookEmission {
            HookEmission {
                exit_code: 1,
                stdout: None,
                stderr: None,
            }
        }
    }

    static FIXTURE_CODEC: FixtureCodec = FixtureCodec;

    fn fixture_harness(id: &str) -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: id.to_owned(),
            codec: &FIXTURE_CODEC,
            hook_config: HarnessHookConfig::new(
                ".fixture/hooks.json",
                |_input: HarnessHookConfigRenderInput<'_>| Ok("{}".to_owned()),
            ),
        })
    }

    #[test]
    fn define_harness_builds_default_targets_and_bindings() {
        let harness = fixture_harness("fixture");
        let target = harness.target(HarnessTargetInput::new("agent-hooks fixture"));
        let binding = harness.bind(HookBindingInput::new(10, "Check Fixture").with_matcher("Bash"));

        assert_eq!(target.harness, "fixture");
        assert_eq!(target.path, ".fixture/hooks.json");
        assert_eq!(target.command_prefix, "agent-hooks fixture");
        assert_eq!(binding.harness, "fixture");
        assert_eq!(binding.matcher.as_deref(), Some("Bash"));
        assert_eq!(binding.timeout, 10);
        assert_eq!(binding.status_message, "Check Fixture");
    }

    #[test]
    fn harness_registries_preserve_codecs_and_hook_configs() {
        let harnesses = vec![fixture_harness("fixture")];
        let codecs = harness_registry_from_harnesses(&harnesses).unwrap();
        let hook_configs = harness_hook_config_registry_from_harnesses(&harnesses).unwrap();

        assert_eq!(codecs["fixture"].harness(), "fixture");
        assert_eq!(hook_configs["fixture"].default_path, ".fixture/hooks.json");
    }

    #[test]
    fn harness_registries_reject_duplicate_ids() {
        let harnesses = vec![fixture_harness("fixture"), fixture_harness("fixture")];

        assert_eq!(
            harness_registry_from_harnesses(&harnesses)
                .err()
                .unwrap()
                .to_string(),
            "Duplicate harness id: fixture"
        );
        assert_eq!(
            harness_hook_config_registry_from_harnesses(&harnesses)
                .unwrap_err()
                .to_string(),
            "Duplicate harness id: fixture"
        );
    }
}
