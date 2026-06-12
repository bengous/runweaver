use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{RunweaverConfig, TaskRunStatus};
use crate::diagnostics::{format_diagnostics, has_error_diagnostics};
use crate::runtime::{
    CreateExecutionContextOptions, compact_run_for_agents, create_execution_context,
    format_notable_runs, is_blocking_run, run_task, task_run_result_label,
};
use crate::surfaces::agent_hooks::{
    AgentHooksConfig, AgentHooksProcessIo, HookEnv, run_agent_hooks_process_main,
};
use anyhow::{Result, anyhow};

use super::{
    RunweaverBinaryManifest, fingerprint_manifest_inputs, read_runweaver_binary_manifest_inputs,
};

pub struct EmbeddedRunweaverRuntime<'config> {
    pub runweaver_config: &'config RunweaverConfig,
    pub agent_hooks_config: &'config AgentHooksConfig<'config>,
    pub manifest: &'config RunweaverBinaryManifest,
}

pub enum EmbeddedRunweaverStdin<'io> {
    Text(&'io str),
    Reader(&'io mut dyn FnMut() -> Result<String>),
}

impl EmbeddedRunweaverStdin<'_> {
    fn read(&mut self) -> Result<String> {
        match self {
            Self::Text(stdin) => Ok((*stdin).to_owned()),
            Self::Reader(read_stdin) => read_stdin(),
        }
    }
}

pub struct EmbeddedRunweaverCliIo<'io> {
    pub stdin: EmbeddedRunweaverStdin<'io>,
    pub stdout: &'io mut dyn Write,
    pub stderr: &'io mut dyn Write,
    pub env: &'io HookEnv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedRunweaverJsonMode {
    Off,
    Compact,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedRunweaverParsedOptions {
    pub cwd: Option<String>,
    pub config_path: Option<String>,
    pub export_name: Option<String>,
    pub json: EmbeddedRunweaverJsonMode,
    pub verbose: bool,
    pub files: Vec<String>,
    pub input_json: Option<String>,
    pub positionals: Vec<String>,
}

pub fn run_embedded_runweaver_cli(
    args: &[String],
    runtime: EmbeddedRunweaverRuntime<'_>,
    io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let command = args.first().map(String::as_str);
    let options = parse_embedded_runweaver_options(&args[1..])?;
    let cwd = absolute_path(options.cwd.as_deref().unwrap_or("."));

    match command {
        Some("check") => match options.positionals.first().map(String::as_str) {
            Some("hooks") => run_check_hooks(&cwd, runtime.agent_hooks_config, io),
            Some("binary") => run_check_binary(&cwd, runtime.manifest, &options, io),
            _ => run_check(&cwd, runtime.runweaver_config, &options, io),
        },
        Some("sync") => run_sync(&cwd, runtime.agent_hooks_config, &options, io),
        Some("hook") => run_hook(runtime.agent_hooks_config, &options, io),
        Some("run") => run_named(runtime.runweaver_config, &options, &cwd, io),
        Some("help") | None => {
            write!(io.stdout, "{}", embedded_runweaver_help_text())?;
            Ok(0)
        }
        Some(command) => {
            write!(
                io.stderr,
                "Unknown embedded runweaver command: {command}\n{}",
                embedded_runweaver_help_text()
            )?;
            Ok(1)
        }
    }
}

pub fn parse_embedded_runweaver_options(args: &[String]) -> Result<EmbeddedRunweaverParsedOptions> {
    let mut positionals = Vec::new();
    let mut files = Vec::new();
    let mut cwd = None;
    let mut config_path = None;
    let mut input_json = None;
    let mut export_name = None;
    let mut json = EmbeddedRunweaverJsonMode::Off;
    let mut verbose = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--cwd" => {
                cwd = args.get(index + 1).cloned();
                index += 2;
            }
            "--config" => {
                config_path = args.get(index + 1).cloned();
                index += 2;
            }
            "--export" => {
                export_name = args.get(index + 1).cloned();
                index += 2;
            }
            "--json" => {
                json = EmbeddedRunweaverJsonMode::Compact;
                index += 1;
            }
            "--input-json" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(anyhow!("Missing --input-json value."));
                };
                input_json = Some(value.clone());
                index += 2;
            }
            "--verbose" => {
                verbose = true;
                index += 1;
            }
            "--file" => {
                if let Some(file) = args.get(index + 1) {
                    files.push(file.clone());
                }
                index += 2;
            }
            "--files" => {
                if let Some(value) = args.get(index + 1) {
                    files.extend(value.split(',').map(str::to_owned));
                }
                index += 2;
            }
            _ if arg.starts_with("--json=") => {
                json = parse_json_mode(&arg["--json=".len()..])?;
                index += 1;
            }
            _ => {
                positionals.push(arg.clone());
                index += 1;
            }
        }
    }

    Ok(EmbeddedRunweaverParsedOptions {
        cwd,
        config_path,
        export_name,
        json,
        verbose,
        files,
        input_json,
        positionals,
    })
}

pub fn embedded_runweaver_help_text() -> &'static str {
    "embedded runweaver <command>\n\nCommands:\n  check               Validate embedded config, scaffold, and managed toolchain\n  check binary        Check embedded binary drift against current source inputs\n  check hooks         Check generated native agent hook config drift\n  sync hooks          Write generated native agent hook configs\n  hook <harness> <command>\n                      Execute an embedded agent hook command from stdin\n  run <task>          Execute an embedded Runweaver task\n\nOptions:\n  --cwd <path>        Project root\n  --json[=compact|full]\n                      Print machine-readable JSON. Compact hides successful output.\n  --verbose           Include successful task stdout/stderr. With --json, emit full JSON.\n  --input-json <json|->\n                      Set ExecutionContext.input from a JSON string or stdin.\n  --file <path>       Add one ExecutionContext file\n  --files <paths>     Add comma-separated ExecutionContext files\n"
}

fn run_check(
    cwd: &Path,
    config: &RunweaverConfig,
    options: &EmbeddedRunweaverParsedOptions,
    io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let diagnostics = crate::config::validate_project(config, cwd);
    if options.json != EmbeddedRunweaverJsonMode::Off {
        writeln!(
            io.stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "diagnostics": diagnostics }))?
        )?;
    } else if diagnostics.is_empty() {
        writeln!(io.stdout, "OK")?;
    } else {
        writeln!(io.stderr, "{}", format_diagnostics(&diagnostics))?;
    }
    Ok(if has_error_diagnostics(&diagnostics) {
        1
    } else {
        0
    })
}

fn run_check_binary(
    cwd: &Path,
    manifest: &RunweaverBinaryManifest,
    options: &EmbeddedRunweaverParsedOptions,
    io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let current_inputs = read_runweaver_binary_manifest_inputs(cwd, &manifest.source_roots)?;
    let current_fingerprint = fingerprint_manifest_inputs(&current_inputs);
    let fresh = current_fingerprint == manifest.fingerprint;
    if options.json != EmbeddedRunweaverJsonMode::Off {
        writeln!(
            io.stdout,
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "fresh": fresh,
                "embedded": manifest,
                "current": {
                    "fingerprint": current_fingerprint,
                    "inputCount": current_inputs.len(),
                    "inputs": current_inputs,
                }
            }))?
        )?;
        return Ok(if fresh { 0 } else { 1 });
    }

    if fresh {
        writeln!(
            io.stdout,
            "OK embedded Runweaver binary fresh ({})",
            manifest.fingerprint
        )?;
        return Ok(0);
    }

    write!(
        io.stderr,
        "Embedded Runweaver binary drift detected.\n  embedded: {}\n  current:  {}\n\nRun:\n  bun run runweaver:compile\n",
        manifest.fingerprint, current_fingerprint
    )?;
    Ok(1)
}

fn run_sync(
    cwd: &Path,
    config: &AgentHooksConfig<'_>,
    options: &EmbeddedRunweaverParsedOptions,
    io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let target = options.positionals.first().map(String::as_str);
    if target != Some("hooks") {
        write!(
            io.stderr,
            "Unknown embedded runweaver sync target: {}\n{}",
            target.unwrap_or(""),
            embedded_runweaver_help_text()
        )?;
        return Ok(1);
    }

    for file in crate::surfaces::agent_hooks::write_harness_hook_config_files(
        cwd,
        &config.harness_hook_config,
    )? {
        writeln!(io.stdout, "Wrote {}", file.path)?;
    }
    Ok(0)
}

fn run_check_hooks(
    cwd: &Path,
    config: &AgentHooksConfig<'_>,
    io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let result = crate::surfaces::agent_hooks::check_harness_hook_config_files(
        cwd,
        &config.harness_hook_config,
    )?;
    if result.ok {
        writeln!(io.stdout, "OK harness hook config files")?;
        return Ok(0);
    }
    for mismatch in result.mismatches {
        let state = if mismatch.actual.is_none() {
            "missing"
        } else {
            "drifted"
        };
        writeln!(io.stderr, "Harness hook config {state}: {}", mismatch.path)?;
    }
    Ok(1)
}

fn run_hook(
    config: &AgentHooksConfig<'_>,
    options: &EmbeddedRunweaverParsedOptions,
    mut io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let stdin = io.stdin.read()?;
    let mut reader = std::io::Cursor::new(stdin);
    run_agent_hooks_process_main(
        &config.app,
        &options.positionals,
        AgentHooksProcessIo {
            stdin: &mut reader,
            env: io.env,
            stdout: io.stdout,
            stderr: io.stderr,
        },
    )
}

fn run_named(
    config: &RunweaverConfig,
    options: &EmbeddedRunweaverParsedOptions,
    cwd: &Path,
    mut io: EmbeddedRunweaverCliIo<'_>,
) -> Result<i32> {
    let Some(task_name) = options.positionals.first() else {
        writeln!(io.stderr, "Missing task name.")?;
        return Ok(1);
    };

    let input = parse_input_json(options, &mut io.stdin)?;
    let mut context_options = CreateExecutionContextOptions::new(path_to_string(cwd));
    context_options.env = Some(hook_env_to_hash_map(io.env));
    context_options.files = options.files.clone();
    context_options.input = input;
    let run = run_task(config, task_name, create_execution_context(context_options))?;

    if options.json != EmbeddedRunweaverJsonMode::Off {
        let payload = if options.json == EmbeddedRunweaverJsonMode::Full || options.verbose {
            serde_json::to_value(&run)?
        } else {
            serde_json::to_value(compact_run_for_agents(&run))?
        };
        writeln!(io.stdout, "{}", serde_json::to_string_pretty(&payload)?)?;
    } else {
        writeln!(
            io.stdout,
            "{}: {}",
            run.task_name,
            task_run_result_label(&run)
        )?;
        if options.verbose && run.status == TaskRunStatus::Completed {
            if let Some(output) = &run.output {
                write!(io.stdout, "{}", output.stdout)?;
                write!(io.stderr, "{}", output.stderr)?;
                if let Some(error) = &output.error {
                    writeln!(io.stderr, "{error}")?;
                }
            }
        } else {
            write!(io.stderr, "{}", format_notable_runs(&run))?;
        }
        if run.status != TaskRunStatus::Completed
            && options.verbose
            && let Some(reason) = &run.reason
        {
            writeln!(io.stderr, "{reason}")?;
        }
    }

    Ok(if is_blocking_run(&run) { 1 } else { 0 })
}

fn parse_input_json(
    options: &EmbeddedRunweaverParsedOptions,
    stdin: &mut EmbeddedRunweaverStdin<'_>,
) -> Result<Option<serde_json::Value>> {
    let Some(input_json) = &options.input_json else {
        return Ok(None);
    };
    let source = if input_json == "-" {
        stdin.read()?
    } else {
        input_json.clone()
    };
    serde_json::from_str(&source)
        .map(Some)
        .map_err(|error| anyhow!("Invalid --input-json payload: {error}"))
}

fn parse_json_mode(value: &str) -> Result<EmbeddedRunweaverJsonMode> {
    match value {
        "compact" => Ok(EmbeddedRunweaverJsonMode::Compact),
        "full" => Ok(EmbeddedRunweaverJsonMode::Full),
        _ => Err(anyhow!("Unsupported --json mode: {value}")),
    }
}

fn hook_env_to_hash_map(env: &HookEnv) -> HashMap<String, String> {
    env.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn absolute_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{
        ActionResult, ActionTask, ExecutionContext, ParallelTask, TaskCompletion, TaskDefinition,
        TaskOutput, ToolDefinition, define_config,
    };
    use crate::surfaces::agent_hooks::{
        AgentHooksConfigDefinition, AgentHooksConfigHook, Harness, HarnessCodec, HarnessDefinition,
        HarnessHookConfig, HarnessHookConfigRenderInput, HarnessTarget, HookBinding,
        HookCommandSpec, HookEmission, HookEvent, HookOutcome, HookRequest, HookStage,
        define_agent_hooks_config, define_harness, hook_groups_by_stage,
    };

    use super::*;

    struct CustomCodec;

    impl HarnessCodec for CustomCodec {
        fn harness(&self) -> &'static str {
            "custom"
        }

        fn decode(&self, stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
            let payload: serde_json::Value = serde_json::from_str(stdin)?;
            Ok(HookRequest {
                event: HookEvent {
                    harness: "custom".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/repo".to_owned(),
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
            HookEmission {
                exit_code: 0,
                stdout: Some(serde_json::to_string(&outcome).unwrap()),
                stderr: None,
            }
        }

        fn encode_failure(&self, _stage: HookStage, error: &anyhow::Error) -> HookEmission {
            HookEmission {
                exit_code: 1,
                stdout: None,
                stderr: Some(format!("{error}\n")),
            }
        }
    }

    static CUSTOM_CODEC: CustomCodec = CustomCodec;

    fn echo_input_action(ctx: &ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Success,
            output: TaskOutput::success(),
            data: ctx.input.clone(),
            next_context: None,
        }
    }

    fn failing_action(_: &ExecutionContext) -> ActionResult {
        ActionResult::Completed {
            completion: TaskCompletion::Error,
            output: TaskOutput {
                exit_code: Some(1),
                stdout: "bad stdout\n".to_owned(),
                stderr: "bad stderr\n".to_owned(),
                error: None,
            },
            data: None,
            next_context: None,
        }
    }

    fn runweaver_config() -> RunweaverConfig {
        define_config(RunweaverConfig {
            tools: HashMap::<String, ToolDefinition>::new(),
            policies: HashMap::new(),
            tasks: HashMap::from([
                (
                    "echoInput".to_owned(),
                    TaskDefinition::Action(ActionTask::new(echo_input_action)),
                ),
                (
                    "fail".to_owned(),
                    TaskDefinition::Action(ActionTask::new(failing_action)),
                ),
                (
                    "check".to_owned(),
                    TaskDefinition::Parallel(ParallelTask {
                        refs: vec!["echoInput".to_owned(), "fail".to_owned()],
                        fail_fast: false,
                        policies: Vec::new(),
                    }),
                ),
            ]),
        })
    }

    fn custom_harness() -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: "custom".to_owned(),
            codec: &CUSTOM_CODEC,
            hook_config: HarnessHookConfig::new(
                ".custom/hooks.json",
                |input: HarnessHookConfigRenderInput<'_>| {
                    Ok(format!(
                        "{}\n",
                        serde_json::to_string_pretty(&hook_groups_by_stage(input.groups)).unwrap()
                    ))
                },
            ),
        })
    }

    fn agent_hooks_config() -> AgentHooksConfig<'static> {
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
                command: HookCommandSpec::new("guard", HookStage::PreTool, |event| {
                    Ok(HookOutcome::block(format!(
                        "blocked {}",
                        event.tool_command.as_deref().unwrap_or("")
                    )))
                })
                .with_harnesses(["custom"]),
                bindings: vec![HookBinding::new("custom", 10, "Guard").with_matcher("Bash")],
            }],
        ))
        .unwrap()
    }

    fn manifest_for(root: &Path, roots: &[String]) -> RunweaverBinaryManifest {
        let inputs = read_runweaver_binary_manifest_inputs(root, roots).unwrap();
        RunweaverBinaryManifest {
            version: super::super::RUNWEAVER_BINARY_MANIFEST_VERSION,
            fingerprint: fingerprint_manifest_inputs(&inputs),
            source_roots: roots.to_vec(),
            input_count: inputs.len(),
            inputs,
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        }
    }

    struct CapturedIo {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        env: HookEnv,
    }

    impl CapturedIo {
        fn new() -> Self {
            Self {
                stdout: Vec::new(),
                stderr: Vec::new(),
                env: HookEnv::new(),
            }
        }

        fn io<'a>(&'a mut self, stdin: &'a str) -> EmbeddedRunweaverCliIo<'a> {
            EmbeddedRunweaverCliIo {
                stdin: EmbeddedRunweaverStdin::Text(stdin),
                stdout: &mut self.stdout,
                stderr: &mut self.stderr,
                env: &self.env,
            }
        }

        fn stdout(&self) -> String {
            String::from_utf8(self.stdout.clone()).unwrap()
        }

        fn stderr(&self) -> String {
            String::from_utf8(self.stderr.clone()).unwrap()
        }
    }

    fn runtime<'a>(
        config: &'a RunweaverConfig,
        hooks: &'a AgentHooksConfig<'a>,
        manifest: &'a RunweaverBinaryManifest,
    ) -> EmbeddedRunweaverRuntime<'a> {
        EmbeddedRunweaverRuntime {
            runweaver_config: config,
            agent_hooks_config: hooks,
            manifest,
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "runweaver-embedded-cli-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn embedded_cli_runs_tasks_with_stdin_json_input_and_full_output() {
        let root = temp_root("run");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let manifest = RunweaverBinaryManifest {
            version: 1,
            fingerprint: "sha256-empty".to_owned(),
            source_roots: Vec::new(),
            input_count: 0,
            inputs: Vec::new(),
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        };
        let mut captured = CapturedIo::new();

        let exit_code = run_embedded_runweaver_cli(
            &args(&[
                "run",
                "echoInput",
                "--cwd",
                root.to_str().unwrap(),
                "--json=full",
                "--input-json",
                "-",
            ]),
            runtime(&config, &hooks, &manifest),
            captured.io(r#"{"path":"src/index.ts"}"#),
        )
        .unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&captured.stdout()).unwrap()["data"],
            serde_json::json!({ "path": "src/index.ts" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_cli_compact_json_hides_successful_children_and_keeps_failures() {
        let root = temp_root("compact");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let manifest = RunweaverBinaryManifest {
            version: 1,
            fingerprint: "sha256-empty".to_owned(),
            source_roots: Vec::new(),
            input_count: 0,
            inputs: Vec::new(),
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        };
        let mut captured = CapturedIo::new();

        let exit_code = run_embedded_runweaver_cli(
            &args(&["run", "check", "--cwd", root.to_str().unwrap(), "--json"]),
            runtime(&config, &hooks, &manifest),
            captured.io(""),
        )
        .unwrap();

        assert_eq!(exit_code, 1);
        let json = serde_json::from_str::<serde_json::Value>(&captured.stdout()).unwrap();
        assert_eq!(json["taskName"], "check");
        assert_eq!(json["completion"], "error");
        assert_eq!(json["output"], serde_json::json!({ "exitCode": 1 }));
        assert_eq!(json["children"][0]["taskName"], "fail");
        assert!(!json.to_string().contains("echoInput"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_cli_syncs_checks_hooks_and_executes_hook_commands() {
        let root = temp_root("hooks");
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let manifest = RunweaverBinaryManifest {
            version: 1,
            fingerprint: "sha256-empty".to_owned(),
            source_roots: Vec::new(),
            input_count: 0,
            inputs: Vec::new(),
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        };

        let mut sync = CapturedIo::new();
        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["sync", "hooks", "--cwd", root.to_str().unwrap()]),
                runtime(&config, &hooks, &manifest),
                sync.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(sync.stdout(), "Wrote .custom/hooks.json\n");
        assert!(
            fs::read_to_string(root.join(".custom/hooks.json"))
                .unwrap()
                .contains("fixture-runweaver custom guard")
        );

        let mut check = CapturedIo::new();
        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["check", "hooks", "--cwd", root.to_str().unwrap()]),
                runtime(&config, &hooks, &manifest),
                check.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(check.stdout(), "OK harness hook config files\n");

        let mut hook = CapturedIo::new();
        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["hook", "custom", "guard", "--cwd", root.to_str().unwrap()]),
                runtime(&config, &hooks, &manifest),
                hook.io(r#"{"command":"rm -rf node_modules"}"#),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&hook.stdout()).unwrap(),
            serde_json::json!({ "status": "block", "reason": "blocked rm -rf node_modules" })
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_cli_checks_binary_freshness() {
        let root = temp_root("binary");
        fs::write(root.join("runweaver.config.ts"), "export default {};\n").unwrap();
        let roots = vec!["runweaver.config.ts".to_owned()];
        let manifest = manifest_for(&root, &roots);
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let mut fresh = CapturedIo::new();

        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["check", "binary", "--cwd", root.to_str().unwrap(), "--json"]),
                runtime(&config, &hooks, &manifest),
                fresh.io(""),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fresh.stdout()).unwrap()["fresh"],
            serde_json::json!(true)
        );

        fs::write(
            root.join("runweaver.config.ts"),
            "export default { changed: true };\n",
        )
        .unwrap();
        let mut drifted = CapturedIo::new();
        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["check", "binary", "--cwd", root.to_str().unwrap()]),
                runtime(&config, &hooks, &manifest),
                drifted.io(""),
            )
            .unwrap(),
            1
        );
        assert!(
            drifted
                .stderr()
                .contains("Embedded Runweaver binary drift detected.")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_cli_renders_help_and_unknown_command_errors() {
        let config = runweaver_config();
        let hooks = agent_hooks_config();
        let manifest = RunweaverBinaryManifest {
            version: 1,
            fingerprint: "sha256-empty".to_owned(),
            source_roots: Vec::new(),
            input_count: 0,
            inputs: Vec::new(),
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        };
        let mut help = CapturedIo::new();
        let mut unknown = CapturedIo::new();

        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["help"]),
                runtime(&config, &hooks, &manifest),
                help.io(""),
            )
            .unwrap(),
            0
        );
        assert!(help.stdout().starts_with("embedded runweaver <command>"));

        assert_eq!(
            run_embedded_runweaver_cli(
                &args(&["wat"]),
                runtime(&config, &hooks, &manifest),
                unknown.io(""),
            )
            .unwrap(),
            1
        );
        assert!(
            unknown
                .stderr()
                .starts_with("Unknown embedded runweaver command: wat")
        );
    }
}
