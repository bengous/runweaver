use anyhow::{Result, anyhow};

use super::contract::{HookEmission, HookEnv, HookOutcome, HookRequest, HookStage};
use super::hook_command::HookCommandSpec;
use super::run_hook::{RunHookInput, run_hook};

/// Protocol translation for one harness: decode its native stdin payload
/// into a [`HookRequest`], encode a [`HookOutcome`] into the native response
/// ([`HookEmission`]), and encode failures without crashing the hook
/// process.
pub trait HarnessCodec: Sync {
    fn harness(&self) -> &'static str;
    fn decode(&self, stdin: &str, stage: HookStage, env: &HookEnv) -> Result<HookRequest>;
    fn encode(&self, outcome: HookOutcome, request: &HookRequest) -> HookEmission;
    fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission;
}

/// Runtime dispatch table for `hook <harness> <command>` invocations: the
/// registered harness codecs and hook commands.
#[derive(Clone)]
pub struct HookApp<'a> {
    pub name: String,
    pub binary_name: String,
    pub harnesses: Vec<&'a dyn HarnessCodec>,
    pub commands: Vec<HookCommandSpec>,
}

impl<'a> HookApp<'a> {
    pub fn harness(&self, harness: &str) -> Result<&'a dyn HarnessCodec> {
        self.harnesses
            .iter()
            .copied()
            .find(|codec| codec.harness() == harness)
            .ok_or_else(|| anyhow!("Unknown hook harness: {harness}"))
    }

    pub fn command(&self, command: &str, harness: &str) -> Result<&HookCommandSpec> {
        let spec = self
            .commands
            .iter()
            .find(|spec| spec.name() == command)
            .ok_or_else(|| anyhow!("Unknown hook command: {command}"))?;
        if !spec.is_enabled_for(harness) {
            return Err(anyhow!(
                "Hook command is not enabled for {harness}: {command}"
            ));
        }
        Ok(spec)
    }

    pub fn usage(&self) -> String {
        let harnesses = join_names(self.harnesses.iter().map(|codec| codec.harness()));
        let commands = join_names(self.commands.iter().map(HookCommandSpec::name));
        let example_harness = self
            .harnesses
            .first()
            .map(|codec| codec.harness())
            .unwrap_or("harness");
        let example_command = self
            .commands
            .first()
            .map(HookCommandSpec::name)
            .unwrap_or("hook-command");
        format!(
            "Usage: {} <{}> <{}>\n\nExamples:\n  {} {} {}",
            self.binary_name,
            harnesses,
            commands,
            self.binary_name,
            example_harness,
            example_command
        )
    }
}

pub fn run_hook_command(
    harness: &str,
    command: &str,
    stdin: &str,
    app: &HookApp<'_>,
) -> Result<HookEmission> {
    run_hook_command_with_env(harness, command, stdin, &HookEnv::new(), app)
}

pub fn run_hook_command_with_env(
    harness: &str,
    command: &str,
    stdin: &str,
    env: &HookEnv,
    app: &HookApp<'_>,
) -> Result<HookEmission> {
    let codec = app.harness(harness)?;
    let spec = app.command(command, harness)?;

    Ok(run_hook(RunHookInput {
        harness: codec,
        command: spec,
        stdin,
        env,
    }))
}

pub fn run_hook_args(args: &[String], stdin: &str, app: &HookApp<'_>) -> Result<HookEmission> {
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => Ok(HookEmission {
            exit_code: 0,
            stdout: Some(format!("{}\n", app.usage())),
            stderr: None,
        }),
        None => Err(anyhow!(app.usage())),
        Some(harness) => {
            let command = args.get(1).map(String::as_str).unwrap_or("");
            run_hook_command(harness, command, stdin, app)
        }
    }
}

pub fn outcome_to_emission(stage: HookStage, outcome: HookOutcome) -> HookEmission {
    match outcome {
        HookOutcome::Pass { system_message, .. } => HookEmission::pass(system_message),
        HookOutcome::Block { reason, .. } => HookEmission::block(stage, reason),
    }
}

fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join("|")
}

#[cfg(test)]
mod tests {
    use super::super::contract::HookEvent;
    use super::*;
    use std::sync::LazyLock;

    struct PlainHarness;

    impl HarnessCodec for PlainHarness {
        fn harness(&self) -> &'static str {
            "plain"
        }

        fn decode(&self, stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
            if stdin == "bad" {
                return Err(anyhow!("decode failed"));
            }
            Ok(HookRequest {
                event: HookEvent {
                    harness: "plain".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/tmp".to_owned(),
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
            assert_eq!(request.event.harness, "plain");
            outcome_to_emission(request.event.stage, outcome)
        }

        fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
            HookEmission::block(stage, error.to_string())
        }
    }

    fn pass_command(event: &HookEvent) -> Result<HookOutcome> {
        Ok(HookOutcome::Pass {
            system_message: event.tool_command.clone(),
            updated_file: None,
        })
    }

    fn failing_command(_: &HookEvent) -> Result<HookOutcome> {
        Err(anyhow!("handler failed"))
    }

    static HARNESS: PlainHarness = PlainHarness;
    static HARNESSES: &[&dyn HarnessCodec] = &[&HARNESS];
    static COMMANDS: LazyLock<Vec<HookCommandSpec>> = LazyLock::new(|| {
        vec![
            HookCommandSpec::new("pass", HookStage::PreTool, pass_command),
            HookCommandSpec::new("fail", HookStage::PreTool, failing_command)
                .with_harnesses(["plain"]),
        ]
    });

    fn app() -> HookApp<'static> {
        HookApp {
            name: "Plain Hooks".to_owned(),
            binary_name: "hooks".to_owned(),
            harnesses: HARNESSES.to_vec(),
            commands: COMMANDS.clone(),
        }
    }

    #[test]
    fn dispatches_command_and_encodes_outcome() {
        let emission = run_hook_command("plain", "pass", "hello", &app()).unwrap();
        assert_eq!(emission.exit_code, 0);
        assert_eq!(emission.stdout.as_deref(), Some("hello\n"));
        assert_eq!(emission.stderr, None);
    }

    #[test]
    fn encodes_decode_and_handler_failures() {
        let decode_failure = run_hook_command("plain", "pass", "bad", &app()).unwrap();
        assert_eq!(decode_failure.exit_code, 2);
        assert_eq!(decode_failure.stderr.as_deref(), Some("decode failed\n"));

        let handler_failure = run_hook_command("plain", "fail", "ok", &app()).unwrap();
        assert_eq!(handler_failure.exit_code, 2);
        assert_eq!(handler_failure.stderr.as_deref(), Some("handler failed\n"));
    }
}
