use std::io::Write;
use std::path::Path;

use anyhow::Result;

use super::agent_hooks_config::AgentHooksConfig;
use super::harness_hook_config::{
    check_harness_hook_config_files, write_harness_hook_config_files,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentHooksConfigCommand {
    Sync,
    Check,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAgentHooksConfigArgs {
    pub command: AgentHooksConfigCommand,
    pub config_path: Option<String>,
    pub export_name: Option<String>,
}

pub struct LoadAgentHooksConfigRequest<'a> {
    pub root: &'a Path,
    pub config_path: &'a str,
    pub export_name: Option<&'a str>,
}

pub struct AgentHooksConfigCliOptions<'io, 'config> {
    pub args: &'io [String],
    pub root: &'io Path,
    pub stdout: &'io mut dyn Write,
    pub stderr: &'io mut dyn Write,
    pub load_config: &'io dyn for<'request> Fn(
        LoadAgentHooksConfigRequest<'request>,
    ) -> Result<AgentHooksConfig<'config>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentHooksConfigCliError {
    #[error("Unknown agent hooks config command: {command}")]
    UnknownCommand { command: String },
    #[error("Missing required flag: {flag}")]
    MissingRequiredFlag { flag: String },
    #[error("Agent hooks command requires a config path.")]
    MissingConfigPath,
}

pub fn run_agent_hooks_config_process_main(
    options: AgentHooksConfigCliOptions<'_, '_>,
) -> Result<i32> {
    let parsed = parse_agent_hooks_config_args(options.args)?;
    match parsed.command {
        AgentHooksConfigCommand::Help => {
            writeln!(options.stdout, "{}", agent_hooks_config_usage())?;
            Ok(0)
        }
        AgentHooksConfigCommand::Sync => {
            let config = load_required_config(&parsed, options.root, options.load_config)?;
            for file in write_harness_hook_config_files(options.root, &config.harness_hook_config)?
            {
                writeln!(options.stdout, "Wrote {}", file.path)?;
            }
            Ok(0)
        }
        AgentHooksConfigCommand::Check => {
            let config = load_required_config(&parsed, options.root, options.load_config)?;
            let result =
                check_harness_hook_config_files(options.root, &config.harness_hook_config)?;
            if result.ok {
                writeln!(options.stdout, "OK harness hook config files")?;
                return Ok(0);
            }
            for mismatch in result.mismatches {
                let state = if mismatch.actual.is_none() {
                    "missing"
                } else {
                    "drifted"
                };
                writeln!(
                    options.stderr,
                    "Harness hook config {state}: {}",
                    mismatch.path
                )?;
            }
            Ok(1)
        }
    }
}

pub fn parse_agent_hooks_config_args(
    args: &[String],
) -> std::result::Result<ParsedAgentHooksConfigArgs, AgentHooksConfigCliError> {
    let command = args.first().map(String::as_str);
    if matches!(command, Some("help" | "--help" | "-h")) {
        return Ok(ParsedAgentHooksConfigArgs {
            command: AgentHooksConfigCommand::Help,
            config_path: None,
            export_name: None,
        });
    }

    let command = match command {
        Some("sync") => AgentHooksConfigCommand::Sync,
        Some("check") => AgentHooksConfigCommand::Check,
        Some(command) => {
            return Err(AgentHooksConfigCliError::UnknownCommand {
                command: command.to_owned(),
            });
        }
        None => {
            return Err(AgentHooksConfigCliError::UnknownCommand {
                command: String::new(),
            });
        }
    };

    Ok(ParsedAgentHooksConfigArgs {
        command,
        config_path: Some(required_flag(args, "--config")?),
        export_name: optional_flag(args, "--export"),
    })
}

pub fn agent_hooks_config_usage() -> &'static str {
    "Usage: runweaver agent-hooks <sync|check> --config <path> [--export <name>]\n\nExamples:\n  runweaver agent-hooks sync --config <config-file>\n  runweaver agent-hooks check --config <config-file> --export agentHooksConfig"
}

fn load_required_config<'config>(
    parsed: &ParsedAgentHooksConfigArgs,
    root: &Path,
    load_config: &dyn for<'request> Fn(
        LoadAgentHooksConfigRequest<'request>,
    ) -> Result<AgentHooksConfig<'config>>,
) -> Result<AgentHooksConfig<'config>> {
    let config_path = parsed
        .config_path
        .as_deref()
        .ok_or(AgentHooksConfigCliError::MissingConfigPath)?;
    load_config(LoadAgentHooksConfigRequest {
        root,
        config_path,
        export_name: parsed.export_name.as_deref(),
    })
}

fn required_flag(
    args: &[String],
    flag: &str,
) -> std::result::Result<String, AgentHooksConfigCliError> {
    optional_flag(args, flag).ok_or_else(|| AgentHooksConfigCliError::MissingRequiredFlag {
        flag: flag.to_owned(),
    })
}

fn optional_flag(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == flag)?;
    let value = args.get(index + 1)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::Result;
    use serde_json::to_string_pretty;

    use super::super::agent_hooks_config::{
        AgentHooksConfigDefinition, AgentHooksConfigHook, define_agent_hooks_config,
    };
    use super::super::contract::{
        HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage,
    };
    use super::super::harness::{Harness, HarnessDefinition, define_harness};
    use super::super::harness_hook_config::{
        HarnessHookConfig, HarnessHookConfigRenderInput, HarnessTarget, HookBinding,
        hook_groups_by_stage,
    };
    use super::super::hook_command::HookCommandSpec;
    use super::super::runtime::HarnessCodec;
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

    static CUSTOM_CODEC: CustomCodec = CustomCodec;

    fn custom_harness() -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: "custom".to_owned(),
            codec: &CUSTOM_CODEC,
            hook_config: HarnessHookConfig::new(
                ".custom/hooks.json",
                |input: HarnessHookConfigRenderInput<'_>| {
                    Ok(format!(
                        "{}\n",
                        to_string_pretty(&hook_groups_by_stage(input.groups)).unwrap()
                    ))
                },
            ),
            agents_surface: crate::surfaces::agent_hooks::AgentsSurfaceDefaults::new(
                crate::surfaces::agent_hooks::RunweaverHookCommandCwd::None,
            ),
        })
    }

    fn fixture_config() -> AgentHooksConfig<'static> {
        let harness = custom_harness();
        define_agent_hooks_config(AgentHooksConfigDefinition::new(
            "fixture-hooks",
            "fixture-runweaver",
            "fixture.config.ts",
            vec![harness.clone()],
            vec![HarnessTarget::new(
                "custom",
                ".custom/hooks.json",
                "fixture-runweaver custom",
            )],
            vec![AgentHooksConfigHook {
                command: HookCommandSpec::new("guard-example", HookStage::PreTool, |_| {
                    Ok(HookOutcome::pass())
                })
                .with_harnesses(["custom"]),
                bindings: vec![HookBinding::new("custom", 10, "Checking").with_matcher("Bash")],
            }],
        ))
        .unwrap()
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "runweaver-agent-hooks-config-cli-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn parse_agent_hooks_config_args_handles_help_and_flags() {
        assert_eq!(
            parse_agent_hooks_config_args(&args(&["--help"]))
                .unwrap()
                .command,
            AgentHooksConfigCommand::Help
        );

        let parsed = parse_agent_hooks_config_args(&args(&[
            "sync",
            "--config",
            " fixture.ts ",
            "--export",
            " named ",
        ]))
        .unwrap();
        assert_eq!(parsed.command, AgentHooksConfigCommand::Sync);
        assert_eq!(parsed.config_path.as_deref(), Some("fixture.ts"));
        assert_eq!(parsed.export_name.as_deref(), Some("named"));
    }

    #[test]
    fn parse_agent_hooks_config_args_reports_unknown_or_missing_config() {
        assert_eq!(
            parse_agent_hooks_config_args(&[])
                .err()
                .unwrap()
                .to_string(),
            "Unknown agent hooks config command: "
        );
        assert_eq!(
            parse_agent_hooks_config_args(&args(&["run"]))
                .err()
                .unwrap()
                .to_string(),
            "Unknown agent hooks config command: run"
        );
        assert_eq!(
            parse_agent_hooks_config_args(&args(&["sync"]))
                .err()
                .unwrap()
                .to_string(),
            "Missing required flag: --config"
        );
    }

    #[test]
    fn run_agent_hooks_config_process_main_syncs_and_checks_native_config_files() {
        let root = temp_root("sync-check");
        let config = fixture_config();
        let load_config = |request: LoadAgentHooksConfigRequest<'_>| {
            assert_eq!(request.root, root.as_path());
            assert_eq!(request.config_path, "fixture.config.ts");
            assert_eq!(request.export_name, None);
            Ok(config.clone())
        };

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let missing = run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
            args: &args(&["check", "--config", "fixture.config.ts"]),
            root: &root,
            stdout: &mut stdout,
            stderr: &mut stderr,
            load_config: &load_config,
        })
        .unwrap();
        assert_eq!(missing, 1);
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "Harness hook config missing: .custom/hooks.json\n"
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let synced = run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
            args: &args(&["sync", "--config", "fixture.config.ts"]),
            root: &root,
            stdout: &mut stdout,
            stderr: &mut stderr,
            load_config: &load_config,
        })
        .unwrap();
        assert_eq!(synced, 0);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "Wrote .custom/hooks.json\n"
        );
        let written = fs::read_to_string(root.join(".custom/hooks.json")).unwrap();
        assert!(written.contains("fixture-runweaver custom guard-example"));

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let ok = run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
            args: &args(&["check", "--config", "fixture.config.ts"]),
            root: &root,
            stdout: &mut stdout,
            stderr: &mut stderr,
            load_config: &load_config,
        })
        .unwrap();
        assert_eq!(ok, 0);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "OK harness hook config files\n"
        );

        fs::write(root.join(".custom/hooks.json"), "{}\n").unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let drifted = run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
            args: &args(&["check", "--config", "fixture.config.ts"]),
            root: &root,
            stdout: &mut stdout,
            stderr: &mut stderr,
            load_config: &load_config,
        })
        .unwrap();
        assert_eq!(drifted, 1);
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "Harness hook config drifted: .custom/hooks.json\n"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn run_agent_hooks_config_process_main_renders_help_without_loading_config() {
        let root = temp_root("help");
        let load_calls = Cell::new(0);
        let load_config = |_request: LoadAgentHooksConfigRequest<'_>| {
            load_calls.set(load_calls.get() + 1);
            Ok(fixture_config())
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run_agent_hooks_config_process_main(AgentHooksConfigCliOptions {
            args: &args(&["help"]),
            root: &root,
            stdout: &mut stdout,
            stderr: &mut stderr,
            load_config: &load_config,
        })
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!("{}\n", agent_hooks_config_usage())
        );
        assert_eq!(load_calls.get(), 0);

        fs::remove_dir_all(root).unwrap();
    }
}
