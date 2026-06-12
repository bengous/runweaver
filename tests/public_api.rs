use std::path::{Path, PathBuf};
use std::sync::Arc;

use runweaver::{
    ActionResult, AgentHooksAppDefinition, AgentHooksConfigBuilder,
    AgentPostEditFeedbackCheckResult, AgentPostEditFeedbackInput, AgentPostEditFeedbackPorts,
    AgentPostEditFeedbackProfileOptions, AgentPostEditFeedbackResult, BindingResolution,
    BindingRunError, BoundOperationRunResult, ChangedFilesOptions, ClockPort, CommandArgs,
    CommandOptions, CompileCargoRunweaverBinaryError, CompileCargoRunweaverBinaryOptions,
    CompileCargoRunweaverBinaryResult, CompiledRunweaverHookCommandOptions,
    CompiledRunweaverProject, CompiledRunweaverProjectBuilder, CompletedActionResultBuilder,
    CreateExecutionContextOptions, EmbeddedRunweaverJsonMode, EmptyScope, EnvPort,
    ExecutionContext, ExitCodeRule, FileSystemPort, FileTargetPolicyOptions, FileTargets,
    FileTargetsOptions, GeneratedFileGuardOptions, GeneratedFileGuardResult, GitPort,
    HarnessTargetInput, HookBindingInput, HookCommandSpec, HookOutcome, HookStage,
    HostCommandDefinition, LogFields, LoggerPort, ManagedToolDefinition, NextExecutionContext,
    OperationDefinition, OperationRef, PolicyVerdict, ProcessRunOptions, ProcessRunOutput,
    ProcessRunnerPort, Profile, ProfileError, ProjectBuildError, ProjectRunweaver,
    RUNWEAVER_BINARY_MANIFEST_VERSION, RUNWEAVER_DEFINITION_MANIFEST_VERSION, ResultMapping,
    RunweaverBinaryManifest, RunweaverBinaryManifestInput, RunweaverConfig, RunweaverConfigBuilder,
    RunweaverDefinitionBuilder, RunweaverDefinitionManifest, RunweaverDefinitionValidation,
    RunweaverDiagnostic, RunweaverDiagnosticSeverity, RunweaverDiagnosticsError,
    RunweaverHookCommandCwd, RunweaverOperationRunError, RunweaverOperationRunResult,
    RunweaverServices, ServicePortError, SessionStatePort, StopSessionFingerprint,
    StopSessionFingerprintResult, StopSessionGeneratedGuardInput, StopSessionGeneratedGuardResult,
    StopSessionValidationBlockedError, StopSessionValidationEnv, StopSessionValidationEnvInput,
    StopSessionValidationInput, StopSessionValidationOptions, StopSessionValidationPorts,
    StopSessionValidationResult, StopSessionValidationRunInput, StopSessionValidationRunResult,
    SurfaceEvent, SurfaceTrigger, TaskCompletion, TaskOutput, TaskPolicies, TempDirectoryOptions,
    TempFileOptions, TempPort, ToolConfig, ToolDefinition, action, agent_hooks_config,
    agent_post_edit_feedback_profile, aggregate_task_completion, aggregate_task_output, allow,
    bind, claude_harness, codex_harness, command, command_with, compile_cargo_runweaver_binary,
    compiled_runweaver_hook_command, compiled_runweaver_project, create_execution_context,
    create_runweaver_definition_manifest, create_stop_session_validation_profile,
    define_agent_hooks_app, define_config, define_config_with, define_operation, define_profile,
    define_runweaver_with, define_surface, deny, error_diagnostic, file_target_policy,
    file_target_verdict, file_targets, fingerprint_manifest_inputs, format_diagnostic,
    format_diagnostics, generated_file_guard, has_error_diagnostics, hook_command,
    hook_command_ref, host_command, is_blocking_run, map_task_completion, normalize_file_path,
    normalize_files, operation_ref, parallel, parse_embedded_runweaver_options, policy, project,
    resolve_binding, run_agent_post_edit_feedback, run_bound_operation,
    run_bound_runweaver_operation, run_resolved_binding, run_resolved_runweaver_binding,
    run_runweaver_operation, run_runweaver_operation_as_json, run_stop_session_validation,
    run_task, series, skip, skip_with_reason, task_ref, tool, tool_ref, validate_binding_registry,
    validate_config, validate_runweaver_definition, warning_diagnostic,
};
use serde_json::Value;

fn ok_action(_: &ExecutionContext) -> ActionResult {
    ActionResult::success()
}

fn allow_policy(_: &ExecutionContext) -> PolicyVerdict {
    allow()
}

#[test]
fn crate_root_exposes_config_and_runtime_authoring_surface() {
    let _completed_builder: CompletedActionResultBuilder = ActionResult::completed();
    let completed = ActionResult::completed()
        .completion(TaskCompletion::Warning)
        .output(TaskOutput::new(Some(2), "warn\n", ""))
        .next_context(NextExecutionContext::new().with_mode("check"))
        .build();
    let ActionResult::Completed {
        completion,
        output,
        next_context,
        ..
    } = completed
    else {
        panic!("completed builder should create a completed action result");
    };
    assert_eq!(completion, TaskCompletion::Warning);
    assert_eq!(output.exit_code, Some(2));
    assert_eq!(
        next_context.and_then(|context| context.mode),
        Some("check".to_owned())
    );

    let mut config = RunweaverConfig::new();
    let mut builder = RunweaverConfigBuilder::new();
    builder.tool("git", ToolDefinition::host_command("git"));
    assert!(builder.build().tools.contains_key("git"));
    let declarative_config = define_config_with(|config| {
        config
            .tool("echo", host_command("echo"))
            .policy("allow", policy(allow_policy))
            .task("ok", action(ok_action));
    });
    assert!(validate_config(&declarative_config).is_empty());

    let host: ToolDefinition = host_command("echo");
    let ToolDefinition::HostCommand(HostCommandDefinition { program }) = &host else {
        panic!("host_command should create a host command definition");
    };
    assert_eq!(program, "echo");
    assert_eq!(
        HostCommandDefinition::new("git"),
        HostCommandDefinition {
            program: "git".to_owned()
        }
    );
    config.tools.insert("echo".to_owned(), host);

    let managed: ToolDefinition = tool("echo", Some(tool_config()));
    let ToolDefinition::Tool(ManagedToolDefinition {
        program,
        config: Some(config_file),
    }) = &managed
    else {
        panic!("tool should create a managed tool definition");
    };
    assert_eq!(program, "echo");
    assert_eq!(config_file.flag, "--config");
    assert_eq!(
        ManagedToolDefinition::new("echo").with_config(tool_config()),
        ManagedToolDefinition {
            program: "echo".to_owned(),
            config: Some(tool_config())
        }
    );
    assert_eq!(
        ToolDefinition::managed_with_config("echo", tool_config()),
        managed
    );
    assert_eq!(
        ToolDefinition::host_command("echo"),
        ToolDefinition::HostCommand(HostCommandDefinition::new("echo"))
    );
    config.tools.insert("managed".to_owned(), managed);
    config
        .policies
        .insert("allow".to_owned(), policy(allow_policy));
    config.tasks.insert("ok".to_owned(), action(ok_action));
    let captured_output = "captured action output\n".to_owned();
    config.tasks.insert(
        "captured".to_owned(),
        action(move |_| {
            ActionResult::completed()
                .output(TaskOutput::new(Some(0), captured_output.clone(), ""))
                .build()
        }),
    );
    config.tasks.insert(
        "echo".to_owned(),
        command("echo", CommandArgs::Static(vec!["hello".to_owned()])),
    );
    config
        .tasks
        .insert("series".to_owned(), series(&["ok"], true));
    config
        .tasks
        .insert("parallel".to_owned(), parallel(&["ok"], false));
    let task_policies: TaskPolicies = vec!["allow".to_owned()];
    assert_eq!(task_policies, vec!["allow".to_owned()]);
    let config = define_config(config);
    let compiled_project_builder: CompiledRunweaverProjectBuilder<'static> =
        compiled_runweaver_project(config.clone());
    let compiled_project: CompiledRunweaverProject<'static> = compiled_project_builder.build();
    assert!(compiled_project.agent_hooks_config().is_none());
    assert!(compiled_project.runweaver_config().tasks.contains_key("ok"));
    assert!(
        compiled_project
            .runweaver_definition()
            .tasks
            .contains_key("ok")
    );
    let _standalone_hooks_builder: AgentHooksConfigBuilder<'static> =
        agent_hooks_config("fixture hooks", "fixture hook", "hooks.rs");
    let compiled_project_with_hooks = compiled_runweaver_project(config.clone())
        .agent_hooks_config_with(
            "fixture hooks",
            "fixture hook",
            "hooks.rs",
            |hooks: &mut AgentHooksConfigBuilder<'static>| {
                let codex = codex_harness();
                hooks.harness(codex.clone());
                hooks.target(codex.target(HarnessTargetInput::new("fixture hook codex")));
                hooks.hook(
                    hook_command(hook_command_ref("guard"), HookStage::PreTool, |_| {
                        Ok(HookOutcome::pass())
                    }),
                    [codex.bind(HookBindingInput::new(10, "Guard").with_matcher("^Bash$"))],
                );
            },
        )
        .expect("compiled project builder should build hook config from closure")
        .build();
    let hooks = compiled_project_with_hooks
        .agent_hooks_config()
        .expect("builder should attach hooks config");
    assert_eq!(hooks.name, "fixture hooks");
    assert!(hooks.app.command("guard", "codex").is_ok());

    assert!(validate_config(&config).is_empty());

    let run = run_task(
        &config,
        "ok",
        create_execution_context(CreateExecutionContextOptions::new(".")),
    )
    .expect("root run_task export should execute action tasks");
    assert!(!is_blocking_run(&run));
    assert_eq!(
        aggregate_task_completion(std::slice::from_ref(&run)),
        TaskCompletion::Success
    );
    assert_eq!(
        aggregate_task_output(std::slice::from_ref(&run), TaskCompletion::Success).exit_code,
        Some(0)
    );
    let captured_run = run_task(
        &config,
        "captured",
        create_execution_context(CreateExecutionContextOptions::new(".")),
    )
    .expect("root action export should accept capturing Rust closures");
    assert_eq!(
        captured_run.output.and_then(|output| {
            if output.stdout == "captured action output\n" {
                Some(output.exit_code)
            } else {
                None
            }
        }),
        Some(Some(0))
    );

    let mapping = ResultMapping {
        success: None,
        warning: Some(vec![2]),
        error: ExitCodeRule::Unset,
        tool_error: ExitCodeRule::Unset,
    };
    assert_eq!(
        map_task_completion(&TaskOutput::error(2, "warn\n"), Some(&mapping)),
        TaskCompletion::Warning
    );

    let targets: FileTargets = file_targets(FileTargetsOptions::new().with_extensions(["rs"]));
    let scoped_ctx = ExecutionContext::new(".").with_files(vec!["src/lib.rs".to_owned()]);
    assert_eq!(
        file_target_verdict(&targets, &scoped_ctx, EmptyScope::Skip, Some("no files")),
        PolicyVerdict::Allow
    );
    let target_policy = file_target_policy(
        targets,
        FileTargetPolicyOptions::new()
            .with_empty_scope(EmptyScope::Skip)
            .with_skip_reason("no files"),
    );
    assert_eq!(
        (target_policy.evaluate)(&ExecutionContext::new(".")),
        PolicyVerdict::Skip {
            reason: Some("no files".to_owned())
        }
    );
    assert_eq!(normalize_file_path("./src\\lib.rs"), "src/lib.rs");
    assert_eq!(normalize_files(".", &Vec::new()), Vec::<String>::new());
    assert_eq!(
        skip(Some("skip")),
        PolicyVerdict::Skip {
            reason: Some("skip".to_owned())
        }
    );
    assert_eq!(skip(None), PolicyVerdict::skip());
    assert_eq!(
        skip_with_reason("skip"),
        PolicyVerdict::skip_with_reason("skip")
    );
    assert_eq!(deny("blocked"), PolicyVerdict::deny("blocked"));
    assert_eq!(
        compiled_runweaver_hook_command(&CompiledRunweaverHookCommandOptions::new(
            "codex",
            RunweaverHookCommandCwd::GitRoot,
            "./.runweaver/bin/demo",
        ))
        .expect("compiled prefix should be valid"),
        "cd \"$(git rev-parse --show-toplevel)\" && ./.runweaver/bin/demo hook codex"
    );
}

#[test]
fn crate_root_exposes_operation_surface_profile_and_binding_composition() {
    let operation = define_operation(
        OperationDefinition::new(|input, _services| {
            let count = input
                .get("files")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Ok(serde_json::json!({ "count": count }))
        })
        .with_description("Count files"),
    );
    let surface = define_surface(
        SurfaceTrigger {
            surface: "agent-hook".to_owned(),
            name: "post-edit".to_owned(),
            phase: Some("after".to_owned()),
        },
        None,
    );
    let profile = define_profile(Profile::new("increment").after_operation(
        |mut output, _context, _input| {
            let count = output.get("count").and_then(Value::as_i64).unwrap_or(0);
            output["count"] = serde_json::json!(count + 1);
            Ok(output)
        },
    ));
    let binding = bind(surface.trigger()).to("countFiles").r#use([profile]);
    let mut definition_builder = RunweaverDefinitionBuilder::new();
    definition_builder
        .tool("cargo", host_command("cargo"))
        .task(
            "validate",
            command("cargo", CommandArgs::Static(vec!["test".to_owned()])),
        )
        .operation("countFiles", operation.clone())
        .binding(binding.clone());
    let built_definition = definition_builder.build();
    let declarative_definition = define_runweaver_with(|runweaver| {
        runweaver
            .tool("cargo", host_command("cargo"))
            .task(
                "validate",
                command("cargo", CommandArgs::Static(vec!["test".to_owned()])),
            )
            .operation("countFiles", operation.clone())
            .binding(binding.clone());
    });
    let event = SurfaceEvent {
        trigger: surface.trigger(),
        payload: serde_json::json!({ "files": ["a.ts", "b.ts"] }),
        metadata: None,
    };
    let run_operation = |operation_name: &str,
                         input: Value,
                         _context: &mut Value|
     -> Result<Value, BindingRunError> {
        assert_eq!(operation_name, "countFiles");
        let count = input
            .get("files")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(serde_json::json!({ "count": count }))
    };
    let mut context = serde_json::json!({});

    let validation =
        validate_binding_registry(std::slice::from_ref(&binding), Some(&["countFiles"]));
    let definition_validation: RunweaverDefinitionValidation =
        validate_runweaver_definition(&built_definition);
    let definition_manifest: RunweaverDefinitionManifest =
        create_runweaver_definition_manifest(&built_definition);
    let method_manifest = built_definition.manifest();
    let _operation_result: Option<RunweaverOperationRunResult> = None;
    let _operation_error: Option<RunweaverOperationRunError> = None;
    let _run_definition_operation = run_runweaver_operation;
    let _run_definition_operation_json = run_runweaver_operation_as_json;
    let _run_definition_binding = run_bound_runweaver_operation;
    let _run_definition_resolved_binding = run_resolved_runweaver_binding;
    let resolution = resolve_binding(std::slice::from_ref(&binding), &event);
    let output = run_bound_operation(
        &binding,
        &run_operation,
        event.payload.clone(),
        &mut context,
    )
    .expect("binding should execute");
    let resolved = run_resolved_binding(&resolution, &run_operation, event.payload, &mut context)
        .expect("resolved binding should execute");

    assert_eq!(operation.description.as_deref(), Some("Count files"));
    assert!(built_definition.tools.contains_key("cargo"));
    assert!(definition_validation.ok());
    assert!(built_definition.validate().ok());
    assert_eq!(
        definition_manifest.version,
        RUNWEAVER_DEFINITION_MANIFEST_VERSION
    );
    assert_eq!(method_manifest, definition_manifest);
    assert!(definition_manifest.operations.contains_key("countFiles"));
    assert!(declarative_definition.tasks.contains_key("validate"));
    assert!(declarative_definition.operations.contains_key("countFiles"));
    assert_eq!(
        declarative_definition.bindings[0].operation_name,
        "countFiles"
    );
    assert!(validation.ok);
    assert!(matches!(resolution, BindingResolution::Matched { .. }));
    assert_eq!(output, serde_json::json!({ "count": 3 }));
    assert_eq!(
        resolved,
        BoundOperationRunResult::Executed {
            output: serde_json::json!({ "count": 3 })
        }
    );
}

#[test]
fn crate_root_exposes_declarative_project_operations_and_bindings() {
    let cargo = tool_ref("cargo");
    let cargo_check = task_ref("cargoCheck");
    let count_files: OperationRef = operation_ref("countFiles");

    let built_project: ProjectRunweaver = project("fixture")
        .tools(|tools| {
            tools.host(cargo, "cargo");
        })
        .tasks(|tasks| {
            tasks.define(
                cargo_check,
                command_with(cargo, CommandOptions::default().args(["check"])),
            );
        })
        .operations(|operations| {
            operations.define(
                count_files,
                OperationDefinition::new(|input, _services| {
                    let count = input
                        .get("files")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Ok(serde_json::json!({ "count": count }))
                }),
            );
        })
        .bindings(|bindings| {
            bindings.bind(
                bind(SurfaceTrigger {
                    surface: "cli".to_owned(),
                    name: "count".to_owned(),
                    phase: None,
                })
                .to(count_files)
                .finish(),
            );
        })
        .build()
        .expect("project builder should preserve operations and bindings");

    assert!(
        built_project
            .task_config()
            .tasks
            .contains_key(cargo_check.as_str())
    );
    assert!(
        built_project
            .runweaver_definition()
            .operations
            .contains_key(count_files.as_str())
    );
    assert_eq!(
        built_project.runweaver_definition().bindings[0].operation_name,
        count_files.as_str()
    );

    let error = project("fixture")
        .bindings(|bindings| {
            bindings.bind(
                bind(SurfaceTrigger {
                    surface: "cli".to_owned(),
                    name: "missing".to_owned(),
                    phase: None,
                })
                .to(count_files)
                .finish(),
            );
        })
        .build()
        .expect_err("missing operation bindings should fail at the build boundary");
    assert!(matches!(error, ProjectBuildError::InvalidBindings { .. }));
}

#[test]
fn crate_root_exposes_agent_hook_app_and_built_in_harnesses() {
    let codex = codex_harness();
    let claude = claude_harness();
    let command =
        HookCommandSpec::new(
            "stop-validate",
            HookStage::Stop,
            |_| Ok(HookOutcome::pass()),
        )
        .with_harnesses(["codex", "claude"]);

    let app = define_agent_hooks_app(AgentHooksAppDefinition {
        name: "Runweaver Hooks".to_owned(),
        binary_name: "runweaver hook".to_owned(),
        harnesses: vec![codex.codec, claude.codec],
        commands: vec![command],
    })
    .expect("agent hook app should be valid");

    assert_eq!(codex.id, "codex");
    assert_eq!(claude.id, "claude");
    assert_eq!(app.name, "Runweaver Hooks");
    assert_eq!(app.commands[0].name(), "stop-validate");
    assert_eq!(app.commands[0].stage(), HookStage::Stop);
}

#[test]
fn crate_root_exposes_profiles_diagnostics_services_and_embedded_exports() {
    let guard = generated_file_guard(
        GeneratedFileGuardOptions::default()
            .with_prefix("dist/")
            .with_reason("generated artifact"),
    );
    let guarded = guard.check("./dist\\index.js");
    assert_eq!(
        guarded,
        GeneratedFileGuardResult::Blocked {
            path: "dist/index.js".to_owned(),
            reason: "generated artifact".to_owned(),
            message: "Blocked generated/protected file: dist/index.js (generated artifact)"
                .to_owned(),
        }
    );

    let feedback_input = AgentPostEditFeedbackInput {
        cwd: "/repo".to_owned(),
        session_id: "session-1".to_owned(),
        touched_path_candidates: vec!["src/lib.rs".to_owned()],
        patch_text: None,
        tool_call_id: None,
    };
    let feedback = run_agent_post_edit_feedback(&feedback_input, &PublicApiFeedbackPorts)
        .expect("public feedback profile runner should execute");
    assert_eq!(feedback, AgentPostEditFeedbackResult::default());
    assert_eq!(
        agent_post_edit_feedback_profile(AgentPostEditFeedbackProfileOptions::new(Arc::new(
            PublicApiFeedbackPorts,
        )))
        .name,
        "agent-post-edit-feedback"
    );

    let stop_input = StopSessionValidationInput {
        cwd: "/repo".to_owned(),
        session_id: "session-1".to_owned(),
        stop_hook_active: None,
        touched_path_candidates: vec!["src/lib.rs".to_owned()],
    };
    let stop_options = StopSessionValidationOptions::new(Arc::new(PublicApiStopPorts))
        .with_validation_env(|input: &StopSessionValidationEnvInput<'_>| {
            assert_eq!(input.run_id, "run-1");
            Ok(StopSessionValidationEnv::from([(
                "RUNWEAVER_PUBLIC_API".to_owned(),
                Some("1".to_owned()),
            )]))
        });
    let stop_result = run_stop_session_validation(&stop_input, &stop_options)
        .expect("public stop-session runner should execute");
    assert_eq!(
        stop_result,
        StopSessionValidationResult::pass_with_message("validated")
    );
    assert_eq!(
        create_stop_session_validation_profile(stop_options).name,
        "stop-session-validation"
    );
    assert_eq!(
        StopSessionValidationBlockedError::new(StopSessionValidationResult::block("blocked"))
            .to_string(),
        "blocked"
    );

    let diagnostic: RunweaverDiagnostic =
        error_diagnostic("RUNWEAVER_PUBLIC_API", "public API check").with_path("src/lib.rs");
    let warning = warning_diagnostic("RUNWEAVER_WARNING", "non-blocking");
    let custom = RunweaverDiagnostic::new(
        "RUNWEAVER_CUSTOM",
        RunweaverDiagnosticSeverity::Warning,
        "custom warning",
    );
    let diagnostic_error =
        RunweaverDiagnosticsError::new("diagnostic failure", vec![diagnostic.clone()]);
    assert!(has_error_diagnostics(&diagnostic_error.diagnostics));
    assert_eq!(
        format_diagnostic(&diagnostic),
        "ERROR RUNWEAVER_PUBLIC_API src/lib.rs: public API check"
    );
    assert_eq!(
        format_diagnostics(&[warning, custom]),
        "WARNING RUNWEAVER_WARNING: non-blocking\nWARNING RUNWEAVER_CUSTOM: custom warning"
    );

    accept_runweaver_services(None);
    accept_file_system_port(None);
    accept_git_port(None);
    accept_process_runner_port(None);
    accept_session_state_port(None);
    accept_logger_port(None);
    accept_env_port(None);
    accept_clock_port(None);
    accept_temp_port(None);
    let service_error = ServicePortError::new("fileSystem", "read failed");
    let log_fields: LogFields = serde_json::Map::new();
    let process_options = ProcessRunOptions::default();
    let process_output = ProcessRunOutput {
        exit_code: Some(0),
        stdout: "ok".to_owned(),
        stderr: String::new(),
        error: None,
    };
    let temp_file_options = TempFileOptions {
        prefix: Some("runweaver-".to_owned()),
        suffix: Some(".tmp".to_owned()),
        contents: Some("body".to_owned()),
    };
    assert_eq!(service_error.to_string(), "fileSystem failed: read failed");
    assert!(ChangedFilesOptions::default().base.is_none());
    assert!(process_options.stdin.is_none());
    assert_eq!(process_output.stdout, "ok");
    assert!(TempDirectoryOptions::default().prefix.is_none());
    assert_eq!(temp_file_options.suffix.as_deref(), Some(".tmp"));
    assert!(log_fields.is_empty());

    let input = RunweaverBinaryManifestInput {
        path: "src/lib.rs".to_owned(),
        size: 12,
        digest: "sha256-input".to_owned(),
    };
    let fingerprint = fingerprint_manifest_inputs(std::slice::from_ref(&input));
    let manifest = RunweaverBinaryManifest {
        version: RUNWEAVER_BINARY_MANIFEST_VERSION,
        fingerprint: fingerprint.clone(),
        source_roots: vec!["src".to_owned()],
        input_count: 1,
        inputs: vec![input],
        built_at: "2026-06-09T00:00:00Z".to_owned(),
    };
    let embedded_options = parse_embedded_runweaver_options(&[
        "--json=full".to_owned(),
        "--file".to_owned(),
        "src/lib.rs".to_owned(),
        "validate".to_owned(),
    ])
    .expect("embedded options should parse");
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.input_count, 1);
    assert_eq!(fingerprint, manifest.fingerprint);
    assert_eq!(embedded_options.json, EmbeddedRunweaverJsonMode::Full);
    assert_eq!(embedded_options.files, vec!["src/lib.rs".to_owned()]);

    let _compile_fn: for<'a> fn(
        CompileCargoRunweaverBinaryOptions<'a>,
    ) -> Result<
        CompileCargoRunweaverBinaryResult,
        CompileCargoRunweaverBinaryError,
    > = compile_cargo_runweaver_binary;
    let compile_options = CompileCargoRunweaverBinaryOptions {
        cwd: Path::new("."),
        package: "runweaver",
        binary_name: "runweaver",
        out_path: ".runweaver/bin/runweaver",
        fingerprint_roots: &["src".to_owned()],
    };
    let compile_result = CompileCargoRunweaverBinaryResult {
        outfile: PathBuf::from(".runweaver/bin/runweaver"),
        manifest,
    };
    assert_eq!(compile_options.package, "runweaver");
    assert_eq!(
        compile_result.outfile,
        PathBuf::from(".runweaver/bin/runweaver")
    );
}

#[test]
fn prelude_exposes_declarative_file_target_helpers() {
    use runweaver::prelude::{
        ActionOptions, CommandOptions, CompositeOptions, EmptyScope, ExecutionContext,
        FileTargetPolicyOptions, FileTargets, FileTargetsOptions, PolicyVerdict, TaskDefinition,
        action_with, command_with, file_target_policy, file_targets, series_with, task_ref,
        tool_ref,
    };

    let targets: FileTargets = file_targets(
        FileTargetsOptions::new()
            .with_extensions(["rs"])
            .with_prefixes(["src"]),
    );
    let policy = file_target_policy(
        targets,
        FileTargetPolicyOptions::new()
            .with_empty_scope(EmptyScope::Skip)
            .with_skip_reason("No Rust source target files."),
    );

    assert_eq!(
        (policy.evaluate)(&ExecutionContext::new(".").with_files(vec!["src/lib.rs".to_owned()])),
        PolicyVerdict::Allow
    );
    assert_eq!(
        (policy.evaluate)(&ExecutionContext::new(".")),
        PolicyVerdict::Skip {
            reason: Some("No Rust source target files.".to_owned())
        }
    );

    let captured_arg_prefix = "--manifest-path=".to_owned();
    let command_task: TaskDefinition = command_with(
        tool_ref("cargo"),
        CommandOptions::default()
            .dynamic_args(move |ctx| vec![format!("{captured_arg_prefix}{}", ctx.cwd)]),
    )
    .into();
    let captured_stdout = "captured action".to_owned();
    let action_task: TaskDefinition = action_with(
        move |_| {
            ActionResult::completed()
                .output(TaskOutput::new(Some(0), captured_stdout.clone(), ""))
                .build()
        },
        ActionOptions::default(),
    )
    .into();
    let series_task: TaskDefinition = series_with(
        [task_ref("cargoCheck")],
        CompositeOptions::default().fail_fast(),
    )
    .into();

    let TaskDefinition::Command(command_task) = command_task else {
        panic!("command_with should produce a command task");
    };
    let CommandArgs::Dynamic(args) = &command_task.args else {
        panic!("dynamic_args should preserve a dynamic args closure");
    };
    assert_eq!(
        args(&ExecutionContext::new("Cargo.toml")),
        vec!["--manifest-path=Cargo.toml"]
    );

    let TaskDefinition::Action(action_task) = action_task else {
        panic!("action_with should produce an action task");
    };
    let ActionResult::Completed { output, .. } = (action_task.run)(&ExecutionContext::new("."))
    else {
        panic!("capturing action closure should complete");
    };
    assert_eq!(output.stdout, "captured action");
    assert!(matches!(
        series_task,
        TaskDefinition::Series(series) if series.fail_fast
    ));
}

fn tool_config() -> ToolConfig {
    ToolConfig::new(".runweaver/configs/tool.json", "--config")
}

fn accept_runweaver_services(_services: Option<RunweaverServices<'_>>) {}

fn accept_file_system_port(_port: Option<&dyn FileSystemPort>) {}

fn accept_git_port(_port: Option<&dyn GitPort>) {}

fn accept_process_runner_port(_port: Option<&dyn ProcessRunnerPort>) {}

fn accept_session_state_port(_port: Option<&dyn SessionStatePort>) {}

fn accept_logger_port(_port: Option<&dyn LoggerPort>) {}

fn accept_env_port(_port: Option<&dyn EnvPort>) {}

fn accept_clock_port(_port: Option<&dyn ClockPort>) {}

fn accept_temp_port(_port: Option<&dyn TempPort>) {}

struct PublicApiFeedbackPorts;

impl AgentPostEditFeedbackPorts for PublicApiFeedbackPorts {
    fn extract_touched_paths(
        &self,
        input: &AgentPostEditFeedbackInput,
    ) -> Result<Vec<String>, ProfileError> {
        Ok(input.touched_path_candidates.clone())
    }

    fn record_touched_paths(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _paths: &[String],
    ) -> Result<(), ProfileError> {
        Ok(())
    }

    fn read_touched_paths(
        &self,
        _input: &AgentPostEditFeedbackInput,
    ) -> Result<Vec<String>, ProfileError> {
        Ok(Vec::new())
    }

    fn normalize_path(
        &self,
        _input: &AgentPostEditFeedbackInput,
        path: &str,
    ) -> Result<Option<String>, ProfileError> {
        Ok(Some(path.to_owned()))
    }

    fn is_inside_project(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _normalized_path: &str,
    ) -> Result<bool, ProfileError> {
        Ok(true)
    }

    fn file_exists(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _normalized_path: &str,
    ) -> Result<bool, ProfileError> {
        Ok(true)
    }

    fn read_text(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _normalized_path: &str,
    ) -> Result<Option<String>, ProfileError> {
        Ok(Some("unchanged".to_owned()))
    }

    fn generated_guard(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _paths: &[String],
    ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError> {
        Ok(AgentPostEditFeedbackCheckResult::passed())
    }

    fn run_operation(
        &self,
        _input: &AgentPostEditFeedbackInput,
        _existing_paths: &[String],
    ) -> Result<AgentPostEditFeedbackCheckResult, ProfileError> {
        Ok(AgentPostEditFeedbackCheckResult::passed())
    }
}

struct PublicApiStopPorts;

impl StopSessionValidationPorts for PublicApiStopPorts {
    fn root(&self, cwd: &str) -> Result<String, ProfileError> {
        Ok(cwd.to_owned())
    }

    fn extract_touched_paths(
        &self,
        input: &StopSessionValidationInput,
        _root: &str,
    ) -> Result<Vec<String>, ProfileError> {
        Ok(input.touched_path_candidates.clone())
    }

    fn read_touched_paths(
        &self,
        _input: &StopSessionValidationInput,
    ) -> Result<Vec<String>, ProfileError> {
        Ok(Vec::new())
    }

    fn clear_touched_paths(&self, _input: &StopSessionValidationInput) -> Result<(), ProfileError> {
        Ok(())
    }

    fn generated_guard(
        &self,
        input: StopSessionGeneratedGuardInput<'_>,
    ) -> Result<StopSessionGeneratedGuardResult, ProfileError> {
        assert_eq!(input.root, "/repo");
        Ok(StopSessionGeneratedGuardResult::allowed())
    }

    fn capture_fingerprint(
        &self,
        _root: &str,
    ) -> Result<StopSessionFingerprintResult, ProfileError> {
        Ok(StopSessionFingerprintResult::captured(
            StopSessionFingerprint {
                signature: "clean".to_owned(),
                paths: Vec::new(),
            },
        ))
    }

    fn run_validation(
        &self,
        input: StopSessionValidationRunInput<'_>,
    ) -> Result<StopSessionValidationRunResult, ProfileError> {
        assert_eq!(
            input.env.get("RUNWEAVER_PUBLIC_API"),
            Some(&Some("1".to_owned()))
        );
        Ok(StopSessionValidationRunResult::accepted_with_message(
            "validated",
        ))
    }

    fn create_id(&self) -> String {
        "run-1".to_owned()
    }
}
