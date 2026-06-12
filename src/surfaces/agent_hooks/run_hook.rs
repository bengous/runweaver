use super::contract::{HookEmission, HookEnv};
use super::hook_command::HookCommandSpec;
use super::runtime::HarnessCodec;

pub struct RunHookInput<'a> {
    pub harness: &'a dyn HarnessCodec,
    pub command: &'a HookCommandSpec,
    pub stdin: &'a str,
    pub env: &'a HookEnv,
}

pub fn run_hook(input: RunHookInput<'_>) -> HookEmission {
    let stage = input.command.stage();
    let request = match input.harness.decode(input.stdin, stage, input.env) {
        Ok(request) => request,
        Err(error) => return input.harness.encode_failure(stage, &error),
    };

    let outcome = match input.command.run(&request.event) {
        Ok(outcome) => outcome,
        Err(error) => return input.harness.encode_failure(stage, &error),
    };

    input.harness.encode(outcome, &request)
}

#[cfg(test)]
mod tests {
    use super::super::contract::{HookEvent, HookOutcome, HookRequest, HookStage};
    use super::*;
    use anyhow::{Result, anyhow};

    struct EnvHarness;

    impl HarnessCodec for EnvHarness {
        fn harness(&self) -> &'static str {
            "env"
        }

        fn decode(&self, stdin: &str, stage: HookStage, env: &HookEnv) -> Result<HookRequest> {
            if stdin == "bad" {
                return Err(anyhow!("decode failed"));
            }
            Ok(HookRequest {
                event: HookEvent {
                    harness: "env".to_owned(),
                    stage,
                    session_id: env
                        .get("RUNWEAVER_SESSION_ID")
                        .cloned()
                        .unwrap_or_else(|| "session".to_owned()),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/repo".to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: Some(stdin.to_owned()),
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(&self, outcome: HookOutcome, request: &HookRequest) -> HookEmission {
            match outcome {
                HookOutcome::Pass { .. } => HookEmission {
                    exit_code: 0,
                    stdout: Some(format!("{}\n", request.event.session_id)),
                    stderr: None,
                },
                HookOutcome::Block { reason, .. } => HookEmission {
                    exit_code: 2,
                    stdout: None,
                    stderr: Some(format!("{reason}\n")),
                },
            }
        }

        fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
            HookEmission::block(stage, error.to_string())
        }
    }

    static ENV_HARNESS: EnvHarness = EnvHarness;

    #[test]
    fn run_hook_decodes_with_env_and_encodes_command_outcome() {
        let command = HookCommandSpec::new("pass", HookStage::PreTool, |_| Ok(HookOutcome::pass()));
        let env = HookEnv::from([(
            "RUNWEAVER_SESSION_ID".to_owned(),
            "session-from-env".to_owned(),
        )]);

        let emission = run_hook(RunHookInput {
            harness: &ENV_HARNESS,
            command: &command,
            stdin: "safe",
            env: &env,
        });

        assert_eq!(emission.exit_code, 0);
        assert_eq!(emission.stdout.as_deref(), Some("session-from-env\n"));
        assert_eq!(emission.stderr, None);
    }

    #[test]
    fn run_hook_routes_decode_and_command_failures_through_harness() {
        let fail_command = HookCommandSpec::new("fail", HookStage::PreTool, |_| {
            Err(anyhow!("handler failed"))
        });
        let env = HookEnv::new();

        let decode_failure = run_hook(RunHookInput {
            harness: &ENV_HARNESS,
            command: &fail_command,
            stdin: "bad",
            env: &env,
        });
        let command_failure = run_hook(RunHookInput {
            harness: &ENV_HARNESS,
            command: &fail_command,
            stdin: "safe",
            env: &env,
        });

        assert_eq!(decode_failure.exit_code, 2);
        assert_eq!(decode_failure.stderr.as_deref(), Some("decode failed\n"));
        assert_eq!(command_failure.exit_code, 2);
        assert_eq!(command_failure.stderr.as_deref(), Some("handler failed\n"));
    }
}
