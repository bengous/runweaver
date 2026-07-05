//! The generic `runweaver` binary entry point: executes a project's
//! `.runweaver/manifest.json` with the library's default builtin registry,
//! with no project-specific Rust code involved.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use runweaver::{HookEnv, RunweaverCliIo, RunweaverStdin, run_generic_runweaver_cli};

fn temp_project(label: &str, manifest: &serde_json::Value) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "runweaver-generic-cli-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join(".runweaver")).expect("temp project root should be created");
    std::fs::write(
        root.join(".runweaver/manifest.json"),
        serde_json::to_string_pretty(manifest).expect("manifest should serialize") + "\n",
    )
    .expect("manifest should be written");
    root
}

fn generic_manifest() -> serde_json::Value {
    serde_json::json!({
        "version": 2,
        "paths": { "writable": ["src/"] },
        "tools": {
            "echoCheck": { "script": "echo generic-manifest-check-ok" }
        },
        "pipelines": {
            "check": { "check": ["echoCheck"] }
        },
        "operations": {},
        "surfaces": {
            "agents": {
                "harnesses": ["claude", "codex"],
                "preTool": [{ "guard": "destructive-commands" }],
                "stop": { "run": "check" }
            },
            "git": {
                "preCommit": { "run": "check" }
            },
            "cli": true
        },
        "bindings": []
    })
}

struct CliRun {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn run_cli(root: &Path, args: &[&str], stdin: &str) -> CliRun {
    let args = args
        .iter()
        .map(|arg| (*arg).to_owned())
        .chain(["--cwd".to_owned(), root.to_string_lossy().into_owned()])
        .collect::<Vec<_>>();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let env = HookEnv::new();
    let exit_code = run_generic_runweaver_cli(
        &args,
        RunweaverCliIo {
            stdin: RunweaverStdin::Text(stdin),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    )
    .expect("generic CLI should not error");
    CliRun {
        exit_code,
        stdout: String::from_utf8(stdout).expect("stdout should be utf8"),
        stderr: String::from_utf8(stderr).expect("stderr should be utf8"),
    }
}

#[cfg(unix)]
fn recorder_manifest(paths: Option<serde_json::Value>) -> serde_json::Value {
    let mut manifest = serde_json::json!({
        "version": 2,
        "tools": {
            "recorder": {
                "check": ["recorder", "--check"],
                "diagnostics": { "parser": "unix" }
            }
        },
        "pipelines": {
            "check": { "check": ["recorder"] }
        },
        "operations": {},
        "surfaces": { "cli": true },
        "bindings": []
    });
    if let Some(paths) = paths {
        manifest
            .as_object_mut()
            .expect("manifest should be a json object")
            .insert("paths".to_owned(), paths);
    }
    manifest
}

#[cfg(unix)]
fn recorder_manifest_with_path_zones() -> serde_json::Value {
    recorder_manifest(Some(serde_json::json!({
        "writable": ["src/"],
        "checkOnly": ["docs/"]
    })))
}

#[cfg(unix)]
fn install_recorder(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = root.join("node_modules").join(".bin");
    std::fs::create_dir_all(&bin_dir).expect("repo-local bin dir should be created");
    let recorder = bin_dir.join("recorder");
    std::fs::write(
        &recorder,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > recorder-args.txt\nexit 0\n",
    )
    .expect("recorder should be written");
    let mut permissions = std::fs::metadata(&recorder)
        .expect("recorder metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&recorder, permissions).expect("recorder should be executable");
}

fn write_project_file(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("project file parent should be created");
    }
    std::fs::write(path, content).expect("project file should be written");
}

fn append_claude_user_hook(root: &Path) {
    let path = root.join(".claude/settings.json");
    let mut settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect(".claude/settings.json should be readable"),
    )
    .expect(".claude/settings.json should parse");
    settings["hooks"]
        .as_object_mut()
        .expect("hooks should be an object")
        .entry("PostToolUse".to_owned())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .expect("PostToolUse should be an array")
        .push(serde_json::json!({
            "matcher": "Edit",
            "hooks": [{
                "type": "command",
                "command": "./scripts/my-hook.sh",
                "timeout": 3,
                "statusMessage": "Mine"
            }]
        }));
    std::fs::write(
        path,
        serde_json::to_string_pretty(&settings).expect("settings should serialize") + "\n",
    )
    .expect(".claude/settings.json should be written");
}

#[cfg(unix)]
fn read_recorder_args(root: &Path) -> String {
    std::fs::read_to_string(root.join("recorder-args.txt"))
        .expect("recorder args should be written")
}

#[test]
fn generic_cli_runs_manifest_pipelines_with_the_default_registry() {
    let root = temp_project("run", &generic_manifest());

    let run = run_cli(&root, &["run", "check"], "");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "run check should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
}

#[cfg(unix)]
#[test]
fn generic_cli_run_check_defaults_file_tool_args_to_declared_path_zones() {
    let root = temp_project("recorder-zones", &recorder_manifest_with_path_zones());
    install_recorder(&root);
    write_project_file(&root, "src/a.ts", "");
    write_project_file(&root, "vendor/b.ts", "");
    write_project_file(&root, ".runweaver/manifest.d.ts", "");

    let run = run_cli(&root, &["run", "check"], "");
    let recorded = read_recorder_args(&root);
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "run check should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert_eq!(recorded, "--check\nsrc/\ndocs/\n");
}

#[cfg(unix)]
#[test]
fn generic_cli_run_check_scopes_explicit_files_to_declared_path_zones() {
    let root = temp_project(
        "recorder-explicit-zones",
        &recorder_manifest_with_path_zones(),
    );
    install_recorder(&root);
    write_project_file(&root, "src/a.ts", "");
    write_project_file(&root, "vendor/b.ts", "");

    let run = run_cli(
        &root,
        &["run", "check", "--files", "src/a.ts,vendor/b.ts"],
        "",
    );
    let recorded = read_recorder_args(&root);
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "run check should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert_eq!(recorded, "--check\nsrc/a.ts\n");
}

#[cfg(unix)]
#[test]
fn generic_cli_run_check_excludes_runweaver_generated_artifacts_from_explicit_files() {
    let root = temp_project(
        "recorder-generated-artifact",
        &recorder_manifest_with_path_zones(),
    );
    install_recorder(&root);
    write_project_file(&root, "src/a.ts", "");
    write_project_file(&root, ".runweaver/manifest.d.ts", "");

    let run = run_cli(
        &root,
        &[
            "run",
            "check",
            "--files",
            ".runweaver/manifest.d.ts,src/a.ts",
        ],
        "",
    );
    let recorded = read_recorder_args(&root);
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "run check should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert_eq!(recorded, "--check\nsrc/a.ts\n");
}

#[cfg(unix)]
#[test]
fn generic_cli_run_check_keeps_whole_tree_behavior_without_declared_paths() {
    let root = temp_project("recorder-no-zones", &recorder_manifest(None));
    install_recorder(&root);
    write_project_file(&root, "src/a.ts", "");
    write_project_file(&root, "vendor/b.ts", "");
    write_project_file(&root, ".runweaver/manifest.d.ts", "");

    let run = run_cli(&root, &["run", "check"], "");
    let recorded = read_recorder_args(&root);
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "run check should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert_eq!(recorded, "--check\n");
}

#[test]
fn generic_cli_syncs_native_hook_configs_and_git_hooks() {
    let root = temp_project("sync", &generic_manifest());

    let run = run_cli(&root, &["sync", "hooks"], "");

    let claude = std::fs::read_to_string(root.join(".claude/settings.json"))
        .expect(".claude/settings.json should be generated");
    let codex = std::fs::read_to_string(root.join(".codex/config.toml"))
        .expect(".codex/config.toml should be generated");
    let pre_commit = std::fs::read_to_string(root.join(".runweaver/git-hooks/pre-commit"))
        .expect("pre-commit git hook should be generated");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "sync hooks should succeed: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert!(
        claude.contains("runweaver hook claude guard-destructive"),
        "claude hooks should invoke the generic binary: {claude}"
    );
    assert!(
        codex.contains("runweaver hook codex guard-destructive"),
        "codex hooks should invoke the generic binary: {codex}"
    );
    assert!(
        pre_commit.contains("exec runweaver git-hook pre-commit"),
        "git pre-commit should fall back to the generic binary on PATH: {pre_commit}"
    );
}

#[test]
fn generic_cli_sync_hooks_preserves_hand_authored_agent_hooks() {
    let root = temp_project("sync-preserve-hooks", &generic_manifest());
    write_project_file(
        &root,
        ".claude/settings.json",
        r#"{
  "permissions": {
    "allow": ["Bash(git status)"]
  },
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "./scripts/my-hook.sh",
            "timeout": 3,
            "statusMessage": "Mine"
          }
        ]
      }
    ]
  }
}
"#,
    );
    write_project_file(
        &root,
        ".codex/config.toml",
        r#"# hand-authored comment
model = "gpt-5.5"

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = './scripts/codex-user-hook.sh'
timeout = 3
statusMessage = "Mine"
"#,
    );

    let sync = run_cli(&root, &["sync", "hooks"], "");
    let check = run_cli(&root, &["check", "hooks"], "");
    let claude = std::fs::read_to_string(root.join(".claude/settings.json"))
        .expect(".claude/settings.json should be readable");
    let codex = std::fs::read_to_string(root.join(".codex/config.toml"))
        .expect(".codex/config.toml should be readable");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        sync.exit_code, 0,
        "sync hooks should succeed: stdout={} stderr={}",
        sync.stdout, sync.stderr
    );
    assert!(
        claude.contains("\"permissions\""),
        "permissions lost: {claude}"
    );
    assert!(
        claude.contains("./scripts/my-hook.sh"),
        "claude user hook lost: {claude}"
    );
    assert!(
        claude.contains("runweaver hook claude guard-destructive"),
        "claude runweaver hook missing: {claude}"
    );
    assert!(
        codex.contains("# hand-authored comment"),
        "codex comment lost: {codex}"
    );
    assert!(codex.contains("model = \"gpt-5.5\""), "model lost: {codex}");
    assert!(
        codex.contains("./scripts/codex-user-hook.sh"),
        "codex user hook lost: {codex}"
    );
    assert!(
        codex.contains("runweaver hook codex guard-destructive"),
        "codex runweaver hook missing: {codex}"
    );
    assert_eq!(
        check.exit_code, 0,
        "check hooks should ignore foreign content: stdout={} stderr={}",
        check.stdout, check.stderr
    );
}

#[test]
fn generic_cli_check_hooks_ignores_foreign_agent_config_content() {
    let root = temp_project("check-ignore-foreign-hooks", &generic_manifest());

    let sync = run_cli(&root, &["sync", "hooks"], "");
    append_claude_user_hook(&root);
    let check = run_cli(&root, &["check", "hooks"], "");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        sync.exit_code, 0,
        "sync hooks should succeed: stdout={} stderr={}",
        sync.stdout, sync.stderr
    );
    assert_eq!(
        check.exit_code, 0,
        "check hooks should ignore foreign content: stdout={} stderr={}",
        check.stdout, check.stderr
    );
}

#[test]
fn generic_cli_dispatches_agent_hooks_through_the_default_registry() {
    let root = temp_project("hook", &generic_manifest());

    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "session-1",
        "transcript_path": "/tmp/transcript.jsonl",
        "cwd": root.to_string_lossy(),
        "tool_use_id": "tool-1",
        "tool_name": "Bash",
        "tool_input": { "command": "git reset --hard" }
    })
    .to_string();
    let blocked = run_cli(&root, &["hook", "claude", "guard-destructive"], &payload);

    let safe_payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "session_id": "session-1",
        "transcript_path": "/tmp/transcript.jsonl",
        "cwd": root.to_string_lossy(),
        "tool_use_id": "tool-1",
        "tool_name": "Bash",
        "tool_input": { "command": "pwd" }
    })
    .to_string();
    let passed = run_cli(
        &root,
        &["hook", "claude", "guard-destructive"],
        &safe_payload,
    );
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        blocked.exit_code, 0,
        "claude pre-tool blocks are emitted as deny payloads: stdout={} stderr={}",
        blocked.stdout, blocked.stderr
    );
    let payload = serde_json::from_str::<serde_json::Value>(&blocked.stdout)
        .expect("claude hook stdout should be json");
    assert_eq!(
        payload["hookSpecificOutput"]["permissionDecision"], "deny",
        "destructive command should be denied: {payload}"
    );
    assert!(
        payload["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap_or_default()
            .contains("Destructive command blocked: git reset --hard"),
        "block reason should reach the harness: {payload}"
    );
    assert_eq!(
        passed.exit_code, 0,
        "safe command should pass: stdout={} stderr={}",
        passed.stdout, passed.stderr
    );
    assert!(
        !passed.stdout.contains("deny"),
        "safe command should not be denied: {}",
        passed.stdout
    );
}

#[test]
fn generic_cli_executes_git_hook_slots_from_the_manifest() {
    let root = temp_project("git-hook", &generic_manifest());

    let run = run_cli(&root, &["git-hook", "pre-commit"], "");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "git-hook pre-commit should run the bound pipeline: stdout={} stderr={}",
        run.stdout, run.stderr
    );
    assert!(
        run.stdout.contains("check"),
        "git hook projection should mention the pipeline: {}",
        run.stdout
    );
}

#[cfg(unix)]
#[test]
fn generic_cli_git_hook_resolves_tools_from_repo_local_node_modules_bin() {
    use std::os::unix::fs::PermissionsExt;

    let manifest = serde_json::json!({
        "version": 2,
        "paths": { "writable": ["src/"] },
        "tools": {
            "hosttool": {
                "check": ["hosttool"],
                "diagnostics": { "parser": "unix" },
                "targets": { "fallback": ["src/"] }
            }
        },
        "pipelines": {
            "check": { "check": ["hosttool"] }
        },
        "operations": {},
        "surfaces": {
            "git": {
                "preCommit": { "run": "check" }
            },
            "cli": true
        },
        "bindings": []
    });
    let root = temp_project("git-hook-repo-local-bin", &manifest);
    let bin_dir = root.join("node_modules").join(".bin");
    std::fs::create_dir_all(&bin_dir).expect("repo-local bin dir should be created");
    let hosttool = bin_dir.join("hosttool");
    std::fs::write(&hosttool, "#!/bin/sh\nexit 0\n").expect("hosttool should be written");
    let mut permissions = std::fs::metadata(&hosttool)
        .expect("hosttool metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&hosttool, permissions).expect("hosttool should be executable");

    let run = run_cli(&root, &["git-hook", "pre-commit"], "");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    assert_eq!(
        run.exit_code, 0,
        "git-hook pre-commit should resolve repo-local hosttool: stdout={} stderr={}",
        run.stdout, run.stderr
    );
}

#[test]
fn generic_cli_requires_a_project_binary_for_unknown_builtins() {
    let mut manifest = generic_manifest();
    manifest["surfaces"]["agents"]["harnesses"] = serde_json::json!(["claude", "acme"]);
    let root = temp_project("unknown-builtin", &manifest);

    let args = vec![
        "run".to_owned(),
        "check".to_owned(),
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let env = HookEnv::new();
    let error = run_generic_runweaver_cli(
        &args,
        RunweaverCliIo {
            stdin: RunweaverStdin::Text(""),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    )
    .expect_err("unknown builtins should fail fast");
    std::fs::remove_dir_all(&root).expect("temp project root should be removed");

    let message = error.to_string();
    assert!(
        message.contains("harness: acme"),
        "error should name the missing builtin: {message}"
    );
    assert!(
        message.contains("project-specific runweaver binary"),
        "error should direct to a project binary: {message}"
    );
}

#[test]
fn generic_cli_reports_a_missing_manifest_clearly() {
    let root = std::env::temp_dir().join(format!(
        "runweaver-generic-cli-missing-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temp root should be created");

    let args = vec![
        "run".to_owned(),
        "check".to_owned(),
        "--cwd".to_owned(),
        root.to_string_lossy().into_owned(),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let env = HookEnv::new();
    let error = run_generic_runweaver_cli(
        &args,
        RunweaverCliIo {
            stdin: RunweaverStdin::Text(""),
            stdout: &mut stdout,
            stderr: &mut stderr,
            env: &env,
        },
    )
    .expect_err("missing manifest should fail fast");
    std::fs::remove_dir_all(&root).expect("temp root should be removed");

    assert!(
        error.to_string().contains(".runweaver/manifest.json"),
        "error should name the manifest path: {error}"
    );
}
