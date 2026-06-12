use std::io::{Read, Write};

use anyhow::Result;

use super::contract::{HookEmission, HookEnv};
use super::runtime::HookApp;

pub struct AgentHooksCliRequest<'a> {
    pub harness: &'a str,
    pub command: &'a str,
    pub stdin: &'a str,
    pub env: &'a HookEnv,
}

pub enum AgentHooksStdin<'a> {
    Text(&'a str),
    Reader(&'a mut dyn FnMut() -> Result<String>),
}

impl AgentHooksStdin<'_> {
    fn read(&mut self) -> Result<String> {
        match self {
            Self::Text(stdin) => Ok((*stdin).to_owned()),
            Self::Reader(read_stdin) => read_stdin(),
        }
    }
}

pub struct AgentHooksCliOptions<'a> {
    pub args: &'a [String],
    pub stdin: AgentHooksStdin<'a>,
    pub env: &'a HookEnv,
}

pub struct AgentHooksProcessIo<'a> {
    pub stdin: &'a mut dyn Read,
    pub env: &'a HookEnv,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
}

pub fn execute_hook_command(
    app: &HookApp<'_>,
    request: AgentHooksCliRequest<'_>,
) -> Result<HookEmission> {
    let codec = app.harness(request.harness)?;
    let command = app.command(request.command, request.harness)?;

    Ok(super::run_hook::run_hook(super::run_hook::RunHookInput {
        harness: codec,
        command,
        stdin: request.stdin,
        env: request.env,
    }))
}

pub fn run_agent_hooks_main(
    app: &HookApp<'_>,
    mut options: AgentHooksCliOptions<'_>,
) -> Result<HookEmission> {
    match options.args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => Ok(HookEmission {
            exit_code: 0,
            stdout: Some(format!("{}\n", app.usage())),
            stderr: None,
        }),
        None => Err(anyhow::anyhow!(app.usage())),
        Some(harness) => {
            let command = options.args.get(1).map(String::as_str).unwrap_or("");
            let stdin = options.stdin.read()?;
            execute_hook_command(
                app,
                AgentHooksCliRequest {
                    harness,
                    command,
                    stdin: &stdin,
                    env: options.env,
                },
            )
        }
    }
}

pub fn run_agent_hooks_process_main(
    app: &HookApp<'_>,
    args: &[String],
    io: AgentHooksProcessIo<'_>,
) -> Result<i32> {
    let AgentHooksProcessIo {
        stdin,
        env,
        stdout,
        stderr,
    } = io;

    let emission = {
        let mut read_process_stdin = || {
            let mut input = String::new();
            stdin.read_to_string(&mut input)?;
            Ok(input)
        };

        run_agent_hooks_main(
            app,
            AgentHooksCliOptions {
                args,
                stdin: AgentHooksStdin::Reader(&mut read_process_stdin),
                env,
            },
        )?
    };

    emission.write_to(stdout, stderr).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::{Result, anyhow};
    use serde_json::json;

    use super::super::app::{AgentHooksAppDefinition, define_agent_hooks_app};
    use super::super::contract::{HookEvent, HookOutcome, HookRequest, HookStage};
    use super::super::hook_command::HookCommandSpec;
    use super::super::runtime::{HarnessCodec, HookApp};
    use super::*;

    struct FixtureHarness;

    impl HarnessCodec for FixtureHarness {
        fn harness(&self) -> &'static str {
            "fixture"
        }

        fn decode(&self, stdin: &str, stage: HookStage, env: &HookEnv) -> Result<HookRequest> {
            let payload: serde_json::Value = serde_json::from_str(stdin)?;
            Ok(HookRequest {
                event: HookEvent {
                    harness: "fixture".to_owned(),
                    stage,
                    session_id: env
                        .get("RUNWEAVER_SESSION_ID")
                        .cloned()
                        .unwrap_or_else(|| "session".to_owned()),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: payload
                        .get("cwd")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("/repo")
                        .to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: payload
                        .get("command")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(&self, outcome: HookOutcome, _request: &HookRequest) -> HookEmission {
            let exit_code = match outcome {
                HookOutcome::Pass { .. } => 0,
                HookOutcome::Block { .. } => 2,
            };
            HookEmission {
                exit_code,
                stdout: Some(serde_json::to_string(&outcome).expect("serialize hook outcome")),
                stderr: None,
            }
        }

        fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
            HookEmission {
                exit_code: 1,
                stdout: None,
                stderr: Some(format!("{}: {error}\n", stage_name(stage))),
            }
        }
    }

    static FIXTURE_HARNESS: FixtureHarness = FixtureHarness;
    static HARNESSES: &[&dyn HarnessCodec] = &[&FIXTURE_HARNESS];

    fn stage_name(stage: HookStage) -> &'static str {
        match stage {
            HookStage::PreTool => "pre-tool",
            HookStage::PostEdit => "post-edit",
            HookStage::Stop => "stop",
        }
    }

    fn guard_command() -> HookCommandSpec {
        HookCommandSpec::new("guard-example", HookStage::PreTool, |event| {
            let command = event
                .tool_command
                .as_deref()
                .ok_or_else(|| anyhow!("missing tool command"))?;
            if command.contains("rm -rf") {
                Ok(HookOutcome::block("destructive command"))
            } else {
                Ok(HookOutcome::pass())
            }
        })
    }

    fn fixture_app(command: HookCommandSpec) -> HookApp<'static> {
        define_agent_hooks_app(AgentHooksAppDefinition {
            name: "Fixture Hooks".to_owned(),
            binary_name: "runweaver hook".to_owned(),
            harnesses: HARNESSES.to_vec(),
            commands: vec![command],
        })
        .unwrap()
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn execute_hook_command_runs_selected_command_through_harness_codec() {
        let app = fixture_app(guard_command());
        let env = HookEnv::from([(
            "RUNWEAVER_SESSION_ID".to_owned(),
            "session-from-env".to_owned(),
        )]);

        let emission = execute_hook_command(
            &app,
            AgentHooksCliRequest {
                harness: "fixture",
                command: "guard-example",
                stdin: r#"{"command":"echo safe"}"#,
                env: &env,
            },
        )
        .unwrap();

        assert_eq!(emission.exit_code, 0);
        assert_eq!(emission.stdout.as_deref(), Some(r#"{"status":"pass"}"#));
    }

    #[test]
    fn run_agent_hooks_main_blocks_and_routes_failures_through_harness_codec() {
        let app = fixture_app(guard_command());
        let env = HookEnv::new();

        let blocked = run_agent_hooks_main(
            &app,
            AgentHooksCliOptions {
                args: &args(&["fixture", "guard-example"]),
                stdin: AgentHooksStdin::Text(r#"{"command":"rm -rf /"}"#),
                env: &env,
            },
        )
        .unwrap();
        assert_eq!(blocked.exit_code, 2);
        assert_eq!(
            blocked.stdout.as_deref(),
            Some(r#"{"status":"block","reason":"destructive command"}"#)
        );

        let failure = run_agent_hooks_main(
            &app,
            AgentHooksCliOptions {
                args: &args(&["fixture", "guard-example"]),
                stdin: AgentHooksStdin::Text("{}"),
                env: &env,
            },
        )
        .unwrap();
        assert_eq!(failure.exit_code, 1);
        assert_eq!(
            failure.stderr.as_deref(),
            Some("pre-tool: missing tool command\n")
        );
    }

    #[test]
    fn run_agent_hooks_process_main_writes_emissions_to_matching_streams() {
        let app = fixture_app(guard_command());
        let args = args(&["fixture", "guard-example"]);
        let env = HookEnv::new();
        let mut stdin = Cursor::new(r#"{"command":"echo safe"}"#);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit_code = run_agent_hooks_process_main(
            &app,
            &args,
            AgentHooksProcessIo {
                stdin: &mut stdin,
                env: &env,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(String::from_utf8(stdout).unwrap(), r#"{"status":"pass"}"#);
        assert_eq!(String::from_utf8(stderr).unwrap(), "");
    }

    #[test]
    fn run_agent_hooks_main_renders_help_without_reading_stdin() {
        let app = fixture_app(guard_command());
        let args = args(&["--help"]);
        let env = HookEnv::new();
        let mut read_count = 0;
        let mut read_stdin = || {
            read_count += 1;
            Ok(json!({"command": "echo safe"}).to_string())
        };

        let emission = run_agent_hooks_main(
            &app,
            AgentHooksCliOptions {
                args: &args,
                stdin: AgentHooksStdin::Reader(&mut read_stdin),
                env: &env,
            },
        )
        .unwrap();

        assert_eq!(emission.exit_code, 0);
        assert_eq!(
            emission.stdout.as_deref(),
            Some(
                "Usage: runweaver hook <fixture> <guard-example>\n\nExamples:\n  runweaver hook fixture guard-example\n"
            )
        );
        assert_eq!(read_count, 0);
    }

    #[test]
    fn run_agent_hooks_main_reports_missing_or_unknown_arguments() {
        let app = fixture_app(guard_command());
        let env = HookEnv::new();

        let missing = run_agent_hooks_main(
            &app,
            AgentHooksCliOptions {
                args: &[],
                stdin: AgentHooksStdin::Text(r#"{"command":"echo safe"}"#),
                env: &env,
            },
        )
        .err()
        .unwrap();
        assert_eq!(
            missing.to_string(),
            "Usage: runweaver hook <fixture> <guard-example>\n\nExamples:\n  runweaver hook fixture guard-example"
        );

        let unknown = run_agent_hooks_main(
            &app,
            AgentHooksCliOptions {
                args: &args(&["fixture", "missing"]),
                stdin: AgentHooksStdin::Text(r#"{"command":"echo safe"}"#),
                env: &env,
            },
        )
        .err()
        .unwrap();
        assert_eq!(unknown.to_string(), "Unknown hook command: missing");
    }
}
