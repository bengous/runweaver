use std::collections::HashSet;

use super::hook_command::{HookCommandError, HookCommandSpec, create_hook_command_catalog};
use super::runtime::{HarnessCodec, HookApp};

pub type AgentHooksApp<'a> = HookApp<'a>;

pub struct AgentHooksAppDefinition<'a> {
    pub name: String,
    pub binary_name: String,
    pub harnesses: Vec<&'a dyn HarnessCodec>,
    pub commands: Vec<HookCommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentHooksAppError {
    #[error("Agent hooks app name must not be empty.")]
    EmptyName,
    #[error("Agent hooks app binaryName must not be empty.")]
    EmptyBinaryName,
    #[error("Agent hooks app must define at least one harness.")]
    EmptyHarnesses,
    #[error("Hook command {command} references unknown harness: {harness}")]
    UnknownCommandHarness { command: String, harness: String },
    #[error("Hook command {command} references duplicate harness: {harness}")]
    DuplicateCommandHarness { command: String, harness: String },
    #[error(transparent)]
    CommandCatalog(#[from] HookCommandError),
}

pub fn define_agent_hooks_app<'a>(
    definition: AgentHooksAppDefinition<'a>,
) -> Result<AgentHooksApp<'a>, AgentHooksAppError> {
    if definition.name.is_empty() {
        return Err(AgentHooksAppError::EmptyName);
    }
    if definition.binary_name.is_empty() {
        return Err(AgentHooksAppError::EmptyBinaryName);
    }
    if definition.harnesses.is_empty() {
        return Err(AgentHooksAppError::EmptyHarnesses);
    }

    let harness_names = definition
        .harnesses
        .iter()
        .map(|harness| harness.harness())
        .collect::<HashSet<_>>();

    for command in &definition.commands {
        let Some(command_harnesses) = command.harnesses() else {
            continue;
        };
        let mut seen_harnesses = HashSet::new();

        for harness in command_harnesses {
            if !harness_names.contains(harness.as_str()) {
                return Err(AgentHooksAppError::UnknownCommandHarness {
                    command: command.name().to_owned(),
                    harness: harness.to_owned(),
                });
            }
            if !seen_harnesses.insert(harness.as_str()) {
                return Err(AgentHooksAppError::DuplicateCommandHarness {
                    command: command.name().to_owned(),
                    harness: harness.to_owned(),
                });
            }
        }
    }

    create_hook_command_catalog(&definition.commands)?;

    Ok(HookApp {
        name: definition.name,
        binary_name: definition.binary_name,
        harnesses: definition.harnesses,
        commands: definition.commands,
    })
}

#[cfg(test)]
mod tests {
    use super::super::contract::{
        HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage,
    };
    use super::*;
    use anyhow::Result;

    struct FixtureHarness;

    impl HarnessCodec for FixtureHarness {
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

    static FIXTURE_HARNESS: FixtureHarness = FixtureHarness;
    static HARNESSES: &[&dyn HarnessCodec] = &[&FIXTURE_HARNESS];

    fn pass_command() -> HookCommandSpec {
        HookCommandSpec::new("guard-example", HookStage::PreTool, |_| {
            Ok(HookOutcome::pass())
        })
    }

    #[test]
    fn define_agent_hooks_app_validates_and_preserves_definition() {
        let commands = vec![pass_command()];
        let app = define_agent_hooks_app(AgentHooksAppDefinition {
            name: "Fixture Hooks".to_owned(),
            binary_name: "runweaver hook".to_owned(),
            harnesses: HARNESSES.to_vec(),
            commands,
        })
        .unwrap();

        assert_eq!(app.name, "Fixture Hooks");
        assert_eq!(app.binary_name, "runweaver hook");
        assert_eq!(app.harnesses.len(), 1);
        assert_eq!(app.commands.len(), 1);
    }

    #[test]
    fn define_agent_hooks_app_rejects_empty_app_boundaries() {
        let commands = vec![pass_command()];

        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: String::new(),
                binary_name: "runweaver hook".to_owned(),
                harnesses: HARNESSES.to_vec(),
                commands: commands.clone(),
            })
            .err()
            .unwrap()
            .to_string(),
            "Agent hooks app name must not be empty."
        );
        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: "Fixture Hooks".to_owned(),
                binary_name: String::new(),
                harnesses: HARNESSES.to_vec(),
                commands: commands.clone(),
            })
            .err()
            .unwrap()
            .to_string(),
            "Agent hooks app binaryName must not be empty."
        );
        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: "Fixture Hooks".to_owned(),
                binary_name: "runweaver hook".to_owned(),
                harnesses: Vec::new(),
                commands,
            })
            .err()
            .unwrap()
            .to_string(),
            "Agent hooks app must define at least one harness."
        );
    }

    #[test]
    fn define_agent_hooks_app_validates_command_harness_references() {
        let unknown = vec![pass_command().with_harnesses(["missing"])];
        let duplicate = vec![pass_command().with_harnesses(["fixture", "fixture"])];

        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: "Fixture Hooks".to_owned(),
                binary_name: "runweaver hook".to_owned(),
                harnesses: HARNESSES.to_vec(),
                commands: unknown,
            })
            .err()
            .unwrap()
            .to_string(),
            "Hook command guard-example references unknown harness: missing"
        );
        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: "Fixture Hooks".to_owned(),
                binary_name: "runweaver hook".to_owned(),
                harnesses: HARNESSES.to_vec(),
                commands: duplicate,
            })
            .err()
            .unwrap()
            .to_string(),
            "Hook command guard-example references duplicate harness: fixture"
        );
    }

    #[test]
    fn define_agent_hooks_app_reuses_command_catalog_validation() {
        let commands = vec![pass_command(), pass_command()];

        assert_eq!(
            define_agent_hooks_app(AgentHooksAppDefinition {
                name: "Fixture Hooks".to_owned(),
                binary_name: "runweaver hook".to_owned(),
                harnesses: HARNESSES.to_vec(),
                commands,
            })
            .err()
            .unwrap()
            .to_string(),
            "Duplicate hook command: guard-example"
        );
    }
}
