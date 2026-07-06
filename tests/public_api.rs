//! Pins the runweaver public API surface: every crate-root re-export, the
//! prelude, and the module paths external consumers rely on. If this file
//! stops compiling, the public surface changed.

use std::path::Path;

use runweaver::{
    AgentsSurfaceDefaults, Binding, BuiltinRegistry, CompileCargoRunweaverBinaryError,
    CompileCargoRunweaverBinaryOptions, CompileCargoRunweaverBinaryResult,
    CompileRunweaverBinaryRequest, CompileRunweaverBinaryResult, CompiledRunweaverProject,
    CompiledRunweaverProjectBuilder, ExecutionContext, Harness, HarnessCodec, HarnessDefinition,
    HarnessHookConfig, HarnessHookConfigError, HarnessHookConfigRegistry,
    HarnessHookConfigRenderInput, HarnessHookConfigSet, HarnessHookGroup, HarnessOptions,
    HarnessTargetInput, HookBindingInput, HookBindingValidationInput, HookConfigCommand,
    HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage, LoadedRunweaverManifest,
    ManifestLoadError, OperationDefinition, Profile, RunweaverCliIo, RunweaverConfig,
    RunweaverDefinition, RunweaverDefinitionManifest, RunweaverHookCommandCwd, RunweaverServices,
    RunweaverStdin, SurfaceTrigger, compile_cargo_runweaver_binary, compiled_runweaver_project,
    default_builtin_registry, define_harness, define_harness_hook_config, define_runweaver_with,
    guard_destructive_command, hook_failure_reason, load_runweaver_manifest, optional_bool,
    optional_string, outcome_to_emission, parse_payload, project, render_harness_hook_config_files,
    require_event_name, require_object, require_present_field, require_string,
    run_compiled_runweaver_project_cli, run_compiled_runweaver_project_cli_with_compile,
    run_generic_runweaver_cli, touched_path_candidates,
};
use serde_json::Value;

fn cli_args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn ok_task_config() -> RunweaverConfig {
    let mut config = RunweaverConfig::new();
    config.tasks.insert(
        "ok".to_owned(),
        runweaver::config::action(|_| runweaver::config::ActionResult::success()),
    );
    config
}

#[test]
fn crate_root_exposes_compiled_project_composition_and_cli() {
    let builder: CompiledRunweaverProjectBuilder<'static> =
        compiled_runweaver_project(ok_task_config());
    let project_root: CompiledRunweaverProject<'static> = builder.build();
    assert!(project_root.runweaver_config().tasks.contains_key("ok"));

    let env: HookEnv = HookEnv::new();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_compiled_runweaver_project_cli(
        &cli_args(&["run", "ok", "--cwd", "."]),
        &project_root,
        RunweaverCliIo {
            stdin: RunweaverStdin::Text(""),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    )
    .expect("compiled project CLI should run action tasks");
    assert_eq!(exit_code, 0);

    let compile = |request: CompileRunweaverBinaryRequest<'_>| -> anyhow::Result<CompileRunweaverBinaryResult> {
        let _ = request.out_path;
        Err(anyhow::anyhow!("compile is not exercised by this pin"))
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code = run_compiled_runweaver_project_cli_with_compile(
        &cli_args(&["run", "ok", "--cwd", "."]),
        &project_root,
        &compile,
        RunweaverCliIo {
            stdin: RunweaverStdin::Text(""),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    )
    .expect("with-compile variant should run tasks without invoking the compiler");
    assert_eq!(exit_code, 0);

    let _generic_cli: fn(&[String], RunweaverCliIo<'_>) -> anyhow::Result<i32> =
        run_generic_runweaver_cli;
    let mut read_stdin = || Ok("{}".to_owned());
    let _reader_stdin = RunweaverStdin::Reader(&mut read_stdin);
}

#[test]
fn crate_root_exposes_definition_authoring_and_manifest_loading() {
    let definition: RunweaverDefinition = define_runweaver_with(|runweaver| {
        runweaver.tool("echo", runweaver::config::host_command("echo"));
    });
    assert!(definition.tools.contains_key("echo"));

    let built = project("fixture")
        .tools(|tools| {
            tools.host(runweaver::config::tool_ref("echo"), "echo");
        })
        .build()
        .expect("project builder should build a tools-only project");
    assert!(built.runweaver_definition().tools.contains_key("echo"));

    let manifest: RunweaverDefinitionManifest = definition.manifest();
    let registry: BuiltinRegistry = default_builtin_registry();
    let loaded: Result<LoadedRunweaverManifest, ManifestLoadError> = load_runweaver_manifest(
        &manifest,
        &registry,
        &runweaver::config::generic_runweaver_project_binary(),
    );
    let loaded = loaded.expect("builtin-free manifest should load against the default registry");
    assert!(loaded.agent_hooks.is_none());
}

#[test]
fn crate_root_exposes_operation_binding_profile_and_service_types() {
    let trigger = SurfaceTrigger {
        surface: "agent-hook".to_owned(),
        name: "post-edit".to_owned(),
        phase: Some("after".to_owned()),
    };
    let binding: Binding = runweaver::bindings::bind(trigger.clone())
        .to("countFiles")
        .finish();
    assert_eq!(binding.trigger, trigger);
    assert_eq!(binding.operation_name, "countFiles");

    let operation: OperationDefinition = OperationDefinition::new(|input, _services| {
        let count = input
            .get("files")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(serde_json::json!({ "count": count }))
    })
    .with_description("Count files");
    assert_eq!(operation.description.as_deref(), Some("Count files"));

    let profile: Profile = Profile::new("noop");
    assert_eq!(profile.name, "noop");

    let context: ExecutionContext = ExecutionContext::new(".");
    assert_eq!(context.cwd, ".");

    fn accept_services(_services: Option<RunweaverServices<'_>>) {}
    accept_services(None);
}

struct PublicApiCodec;

static PUBLIC_API_CODEC: PublicApiCodec = PublicApiCodec;

impl HarnessCodec for PublicApiCodec {
    fn harness(&self) -> &'static str {
        "public-api"
    }

    fn decode(&self, stdin: &str, stage: HookStage, env: &HookEnv) -> anyhow::Result<HookRequest> {
        let _ = env;
        let payload = parse_payload(stdin, "public-api")?;
        let session_id = require_string(&payload, "session_id", "public-api")?;
        let cwd = require_string(&payload, "cwd", "public-api")?;
        let tool_input = require_object(&payload, "tool_input", "public-api")?;
        let _present = require_present_field(&payload, "cwd", "public-api")?;
        let tool_command = optional_string(&payload, "command", "public-api")?;
        let stop_hook_active = optional_bool(&payload, "stop_hook_active", "public-api")?;
        let _event_name_contract = require_event_name(&payload, "public-api", stage);
        Ok(HookRequest {
            event: HookEvent {
                harness: "public-api".to_owned(),
                stage,
                session_id,
                tool_call_id: None,
                transcript_path: None,
                cwd,
                touched_path_candidates: touched_path_candidates(tool_input),
                patch_text: None,
                tool_command,
                tool_name: None,
                tool_response: None,
                stop_hook_active: stop_hook_active.unwrap_or(false),
            },
        })
    }

    fn encode(&self, outcome: HookOutcome, request: &HookRequest) -> HookEmission {
        outcome_to_emission(request.event.stage, outcome)
    }

    fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
        HookEmission {
            exit_code: 2,
            stdout: None,
            stderr: Some(hook_failure_reason(stage, error)),
        }
    }
}

#[test]
fn crate_root_exposes_harness_codec_contract_and_payload_helpers() {
    let request = PUBLIC_API_CODEC
        .decode(
            r#"{"session_id":"s-1","cwd":".","command":"rm -rf /","tool_input":{"file_path":"src/lib.rs"}}"#,
            HookStage::PreTool,
            &HookEnv::new(),
        )
        .expect("codec should decode a payload with the public helpers");
    assert_eq!(
        request.event.touched_path_candidates,
        vec!["src/lib.rs".to_owned()]
    );
    assert_eq!(request.event.tool_command.as_deref(), Some("rm -rf /"));

    assert!(guard_destructive_command("rm -rf /").is_some());
    assert!(
        runweaver::surfaces::agent_hooks::guard_destructive_command("cargo check").is_none(),
        "module path to the agent-hook surface must stay public"
    );

    let emission = PUBLIC_API_CODEC.encode(HookOutcome::pass(), &request);
    assert_eq!(emission.exit_code, 0);
    let failure = PUBLIC_API_CODEC.encode_failure(HookStage::Stop, &anyhow::anyhow!("boom"));
    assert!(
        failure
            .stderr
            .as_deref()
            .is_some_and(|reason| reason.contains("boom"))
    );
}

fn render_public_api_hook_config(
    input: HarnessHookConfigRenderInput<'_>,
) -> Result<String, HarnessHookConfigError> {
    let groups: &[HarnessHookGroup] = input.groups;
    let stages: Vec<&str> = groups.iter().map(|group| group.stage.as_str()).collect();
    Ok(format!("// {}\n{}\n", input.source_path, stages.join(",")))
}

fn validate_public_api_binding(
    input: HookBindingValidationInput<'_>,
) -> Result<(), HarnessHookConfigError> {
    if input.binding.options.contains_key("unsupported") {
        return Err(HarnessHookConfigError::Custom {
            message: format!(
                "Hook command {} uses an unsupported option.",
                input.hook.name
            ),
        });
    }
    Ok(())
}

#[test]
fn crate_root_exposes_custom_harness_definition_and_hook_config_rendering() {
    let harness: Harness<'static> = define_harness(HarnessDefinition {
        id: "public-api".to_owned(),
        codec: &PUBLIC_API_CODEC,
        hook_config: define_harness_hook_config(
            HarnessHookConfig::new(".public-api/hooks.json", render_public_api_hook_config)
                .with_validate_binding(validate_public_api_binding),
        ),
        agents_surface: AgentsSurfaceDefaults::new(RunweaverHookCommandCwd::GitRoot)
            .with_stop_status("Running validation"),
    });
    assert_eq!(harness.id, "public-api");

    let mut binding_options: HarnessOptions = HarnessOptions::new();
    binding_options.insert("runWhen".to_owned(), serde_json::json!("always"));
    let set = HarnessHookConfigSet {
        source_path: "hooks.rs".to_owned(),
        hook_configs: HarnessHookConfigRegistry::from([(
            "public-api".to_owned(),
            harness.hook_config.clone(),
        )]),
        targets: vec![harness.target(HarnessTargetInput::new("./bin hook public-api"))],
        hooks: vec![HookConfigCommand::new(
            "stop-validate",
            HookStage::Stop,
            vec![harness.bind(
                HookBindingInput::new(240, "Running validation").with_options(binding_options),
            )],
        )],
    };

    let files =
        render_harness_hook_config_files(&set).expect("render should produce hook config files");
    assert_eq!(files[0].path, ".public-api/hooks.json");
    assert!(files[0].content.contains("// hooks.rs"));
}

#[test]
fn crate_root_exposes_project_binary_compilation_exports() {
    let _compile: for<'a> fn(
        CompileCargoRunweaverBinaryOptions<'a>,
    ) -> Result<
        CompileCargoRunweaverBinaryResult,
        CompileCargoRunweaverBinaryError,
    > = compile_cargo_runweaver_binary;
    let options = CompileCargoRunweaverBinaryOptions {
        cwd: Path::new("."),
        package: "runweaver",
        binary_name: "runweaver",
        out_path: ".runweaver/bin/runweaver",
        fingerprint_roots: &["src".to_owned()],
    };
    assert_eq!(options.package, "runweaver");
}

#[test]
fn prelude_exposes_the_authoring_core() {
    use runweaver::prelude::{
        BuiltinRegistry, CompiledRunweaverProject, CompiledRunweaverProjectBuilder,
        ExecutionContext, LoadedRunweaverManifest, ManifestLoadError, RunweaverConfig,
        RunweaverDefinition, RunweaverDefinitionManifest, RunweaverProjectBinary,
        compiled_runweaver_project, default_builtin_registry, define_runweaver_with,
        load_runweaver_manifest, project,
    };

    let binary = RunweaverProjectBinary {
        package: "fixture".to_owned(),
        binary_name: "fixture".to_owned(),
        out_path: ".runweaver/bin/fixture".to_owned(),
        hooks_config_name: "fixture-hooks".to_owned(),
        fallback_command: "cargo run -p fixture --".to_owned(),
        hook_bin: "./.runweaver/bin/fixture".to_owned(),
    };
    let definition: RunweaverDefinition = define_runweaver_with(|_| {});
    let manifest: RunweaverDefinitionManifest = definition.manifest();
    let registry: BuiltinRegistry = default_builtin_registry();
    let loaded: Result<LoadedRunweaverManifest, ManifestLoadError> =
        load_runweaver_manifest(&manifest, &registry, &binary);
    let loaded = loaded.expect("empty manifest should load against the default registry");
    let config: RunweaverConfig = loaded.definition.task_config();
    assert!(config.tasks.is_empty());
    let context: ExecutionContext = ExecutionContext::new(".");
    assert_eq!(context.cwd, ".");
    let builder: CompiledRunweaverProjectBuilder<'static> =
        compiled_runweaver_project(loaded.definition);
    let compiled: CompiledRunweaverProject<'static> = builder.build();
    assert!(compiled.agent_hooks_config().is_none());
    let _project_entry = project("fixture");
}

#[test]
fn modules_expose_task_authoring_and_runtime_used_by_consumers() {
    use runweaver::config::{
        ActionResult, ExecutionContext, FileTargets, FileTargetsOptions, RunweaverConfig,
        TaskCompletion, TaskDefinition, TaskRunStatus, action, file_targets, parallel, series,
    };
    use runweaver::runtime::run_task;

    let mut config = RunweaverConfig::new();
    config
        .tasks
        .insert("ok".to_owned(), action(|_| ActionResult::success()));
    config
        .tasks
        .insert("series".to_owned(), series(&["ok"], true));
    config
        .tasks
        .insert("parallel".to_owned(), parallel(&["ok"], false));
    assert!(matches!(
        config.tasks.get("parallel"),
        Some(TaskDefinition::Parallel(_))
    ));
    assert!(config.tools.is_empty());
    assert!(config.policies.is_empty());

    let run = run_task(&config, "ok", ExecutionContext::new(".")).expect("action task should run");
    assert_eq!(run.status, TaskRunStatus::Completed);
    assert_ne!(run.status, TaskRunStatus::Skipped);
    assert!(run.reason.is_none());
    if let Some(output) = run.output {
        let _ = (output.stdout, output.exit_code);
    }

    let series_run =
        run_task(&config, "series", ExecutionContext::new(".")).expect("series task should run");
    assert_ne!(series_run.status, TaskRunStatus::Skipped);

    let _targets: FileTargets = file_targets(FileTargetsOptions::new().with_extensions(["rs"]));
    let _completion: TaskCompletion = TaskCompletion::Success;
}
