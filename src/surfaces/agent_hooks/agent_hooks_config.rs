use super::app::{
    AgentHooksApp, AgentHooksAppDefinition, AgentHooksAppError, define_agent_hooks_app,
};
use super::harness::{
    Harness, HarnessError, HarnessRegistry, harness_hook_config_registry_from_harnesses,
    harness_registry_from_harnesses,
};
use super::harness_hook_config::{
    HarnessHookConfigError, HarnessHookConfigSet, HarnessTarget, HookBinding, HookConfigCommand,
    validate_harness_hook_config_set,
};
use super::hook_command::HookCommandSpec;
use super::runtime::HarnessCodec;

#[derive(Debug, Clone)]
pub struct AgentHooksConfigHook {
    pub command: HookCommandSpec,
    pub bindings: Vec<HookBinding>,
}

pub fn define_hook(definition: AgentHooksConfigHook) -> AgentHooksConfigHook {
    definition
}

#[derive(Clone)]
pub struct AgentHooksConfigDefinition<'a> {
    pub name: String,
    pub binary_name: String,
    pub source_path: String,
    pub harnesses: Vec<Harness<'a>>,
    pub targets: Vec<HarnessTarget>,
    pub hooks: Vec<AgentHooksConfigHook>,
}

impl<'a> AgentHooksConfigDefinition<'a> {
    pub fn new(
        name: impl Into<String>,
        binary_name: impl Into<String>,
        source_path: impl Into<String>,
        harnesses: Vec<Harness<'a>>,
        targets: Vec<HarnessTarget>,
        hooks: Vec<AgentHooksConfigHook>,
    ) -> Self {
        Self {
            name: name.into(),
            binary_name: binary_name.into(),
            source_path: source_path.into(),
            harnesses,
            targets,
            hooks,
        }
    }
}

/// The fully composed agent-hook surface: harness registry, target config
/// files, hook commands with their bindings, the runtime dispatch app, and
/// the per-harness config renderers. Built and validated by
/// [`define_agent_hooks_config`].
#[derive(Clone)]
pub struct AgentHooksConfig<'a> {
    pub name: String,
    pub binary_name: String,
    pub source_path: String,
    pub harnesses: HarnessRegistry<'a>,
    pub targets: Vec<HarnessTarget>,
    pub hooks: Vec<AgentHooksConfigHook>,
    pub app: AgentHooksApp<'a>,
    pub harness_hook_config: HarnessHookConfigSet,
}

impl std::fmt::Debug for AgentHooksConfig<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentHooksConfig")
            .field("name", &self.name)
            .field("binary_name", &self.binary_name)
            .field("source_path", &self.source_path)
            .field("harnesses", &self.harnesses.keys().collect::<Vec<_>>())
            .field("targets", &self.targets)
            .field("hooks", &self.hooks)
            .field("app", &"<app>")
            .field("harness_hook_config", &self.harness_hook_config)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentHooksConfigError {
    #[error(transparent)]
    Harness(#[from] HarnessError),
    #[error(transparent)]
    HarnessHookConfig(#[from] HarnessHookConfigError),
    #[error(transparent)]
    App(#[from] AgentHooksAppError),
    #[error("Hook command {command} must define at least one binding.")]
    EmptyHookBindings { command: String },
    #[error("Hook command {command} has duplicate binding for {harness}.")]
    DuplicateHookBinding { command: String, harness: String },
    #[error(
        "Hook command {command} harnesses differ from bindings: {command_harnesses} != {binding_harnesses}."
    )]
    CommandHarnessMismatch {
        command: String,
        command_harnesses: String,
        binding_harnesses: String,
    },
}

pub fn define_agent_hooks_config<'a>(
    definition: AgentHooksConfigDefinition<'a>,
) -> Result<AgentHooksConfig<'a>, AgentHooksConfigError> {
    let harnesses = harness_registry_from_harnesses(&definition.harnesses)?;
    let hook_configs = harness_hook_config_registry_from_harnesses(&definition.harnesses)?;
    let harness_hook_config = harness_hook_config_set(&definition, hook_configs);
    validate_harness_hook_config_set(&harness_hook_config)?;

    let commands = definition
        .hooks
        .iter()
        .map(|hook| hook_command_with_bound_harnesses(&hook.command, &hook.bindings))
        .collect::<Result<Vec<_>, _>>()?;
    let app_harnesses = definition
        .harnesses
        .iter()
        .map(|harness| harness.codec)
        .collect::<Vec<&dyn HarnessCodec>>();
    let app = define_agent_hooks_app(AgentHooksAppDefinition {
        name: definition.name.clone(),
        binary_name: definition.binary_name.clone(),
        harnesses: app_harnesses,
        commands,
    })?;

    Ok(AgentHooksConfig {
        name: definition.name,
        binary_name: definition.binary_name,
        source_path: definition.source_path,
        harnesses,
        targets: definition.targets,
        hooks: definition.hooks,
        app,
        harness_hook_config,
    })
}

fn harness_hook_config_set(
    definition: &AgentHooksConfigDefinition<'_>,
    hook_configs: super::harness_hook_config::HarnessHookConfigRegistry,
) -> HarnessHookConfigSet {
    HarnessHookConfigSet {
        source_path: definition.source_path.clone(),
        hook_configs,
        targets: definition.targets.clone(),
        hooks: definition
            .hooks
            .iter()
            .map(|hook| {
                HookConfigCommand::new(
                    hook.command.name(),
                    hook.command.stage(),
                    hook.bindings.clone(),
                )
            })
            .collect(),
    }
}

fn hook_command_with_bound_harnesses(
    command: &HookCommandSpec,
    bindings: &[HookBinding],
) -> Result<HookCommandSpec, AgentHooksConfigError> {
    let bound_harnesses = unique_harnesses(command.name(), bindings)?;
    validate_command_harnesses(command, &bound_harnesses)?;
    Ok(command.clone().with_harnesses(bound_harnesses))
}

fn unique_harnesses(
    command_name: &str,
    bindings: &[HookBinding],
) -> Result<Vec<String>, AgentHooksConfigError> {
    let mut harnesses = Vec::new();
    for binding in bindings {
        if harnesses.iter().any(|harness| harness == &binding.harness) {
            return Err(AgentHooksConfigError::DuplicateHookBinding {
                command: command_name.to_owned(),
                harness: binding.harness.clone(),
            });
        }
        harnesses.push(binding.harness.clone());
    }
    if harnesses.is_empty() {
        return Err(AgentHooksConfigError::EmptyHookBindings {
            command: command_name.to_owned(),
        });
    }
    Ok(harnesses)
}

fn validate_command_harnesses(
    command: &HookCommandSpec,
    bound_harnesses: &[String],
) -> Result<(), AgentHooksConfigError> {
    let Some(command_harnesses) = command.harnesses() else {
        return Ok(());
    };

    let command_key = harness_key(command_harnesses);
    let binding_key = harness_key(bound_harnesses);
    if command_key != binding_key {
        return Err(AgentHooksConfigError::CommandHarnessMismatch {
            command: command.name().to_owned(),
            command_harnesses: command_key,
            binding_harnesses: binding_key,
        });
    }
    Ok(())
}

fn harness_key(harnesses: &[String]) -> String {
    let mut sorted = harnesses.to_vec();
    sorted.sort_unstable();
    sorted.join(",")
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::super::contract::{
        HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage,
    };
    use super::super::harness::{
        HarnessDefinition, HarnessTargetInput, HookBindingInput, define_harness,
    };
    use super::super::harness_hook_config::{
        HarnessHookConfig, HarnessHookConfigRenderInput, hook_groups_by_stage,
    };
    use super::*;

    struct CustomCodec;

    impl HarnessCodec for CustomCodec {
        fn harness(&self) -> &'static str {
            "custom"
        }

        fn decode(&self, _stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
            Ok(HookRequest {
                event: HookEvent {
                    harness: "custom".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/fixture".to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: None,
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(&self, outcome: HookOutcome, _request: &HookRequest) -> HookEmission {
            HookEmission {
                exit_code: 0,
                stdout: Some(format!("{outcome:?}")),
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

    static CUSTOM_CODEC: CustomCodec = CustomCodec;

    fn custom_harness() -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: "custom".to_owned(),
            codec: &CUSTOM_CODEC,
            hook_config: HarnessHookConfig::new(
                ".custom/hooks.json",
                |input: HarnessHookConfigRenderInput<'_>| {
                    Ok(format!("{:?}", hook_groups_by_stage(input.groups)))
                },
            ),
        })
    }

    fn guard_command() -> HookCommandSpec {
        HookCommandSpec::new("guard-example", HookStage::PreTool, |_| {
            Ok(HookOutcome::pass())
        })
        .with_harnesses(["custom"])
    }

    fn fixture_config() -> AgentHooksConfig<'static> {
        let harness = custom_harness();
        let target = harness.target(HarnessTargetInput::new("fixture-runweaver custom"));
        let binding = harness.bind(HookBindingInput::new(10, "Checking").with_matcher("Bash"));
        define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness],
            vec![target],
            vec![define_hook(AgentHooksConfigHook {
                command: guard_command(),
                bindings: vec![binding],
            })],
        ))
        .unwrap()
    }

    #[test]
    fn composes_runtime_app_and_native_hook_config_from_bindings() {
        let config = fixture_config();

        assert_eq!(
            config
                .app
                .command("guard-example", "custom")
                .unwrap()
                .harnesses()
                .unwrap(),
            &["custom".to_owned()]
        );
        assert_eq!(
            config.harness_hook_config.hooks,
            vec![HookConfigCommand::new(
                "guard-example",
                HookStage::PreTool,
                vec![HookBinding::new("custom", 10, "Checking").with_matcher("Bash")]
            )]
        );
    }

    #[test]
    fn rejects_duplicate_or_empty_hook_bindings() {
        let harness = custom_harness();
        let duplicate_binding =
            harness.bind(HookBindingInput::new(10, "Checking").with_matcher("Bash"));
        let duplicate = define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness.clone()],
            vec![harness.target(HarnessTargetInput::new("fixture-runweaver custom"))],
            vec![AgentHooksConfigHook {
                command: guard_command(),
                bindings: vec![duplicate_binding.clone(), duplicate_binding],
            }],
        ))
        .unwrap_err();

        assert_eq!(
            duplicate.to_string(),
            "Hook command guard-example has duplicate binding for custom."
        );

        let empty = define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness.clone()],
            vec![harness.target(HarnessTargetInput::new("fixture-runweaver custom"))],
            vec![AgentHooksConfigHook {
                command: guard_command(),
                bindings: Vec::new(),
            }],
        ))
        .unwrap_err();

        assert_eq!(
            empty.to_string(),
            "Hook command guard-example must define at least one binding."
        );
    }

    #[test]
    fn rejects_command_harnesses_that_differ_from_bindings() {
        let harness = custom_harness();
        let error = define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness.clone()],
            vec![harness.target(HarnessTargetInput::new("fixture-runweaver custom"))],
            vec![AgentHooksConfigHook {
                command: HookCommandSpec::new("guard-example", HookStage::PreTool, |_| {
                    Ok(HookOutcome::pass())
                })
                .with_harnesses(["other"]),
                bindings: vec![harness.bind(HookBindingInput::new(10, "Checking"))],
            }],
        ))
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Hook command guard-example harnesses differ from bindings: other != custom."
        );
    }
}
