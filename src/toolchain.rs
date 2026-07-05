//! The managed `.runweaver/` toolchain directory.
//!
//! Runweaver-managed tools live in a project-local `.runweaver/` directory
//! with its own `package.json` and `node_modules`, so tool versions are
//! pinned per project instead of depending on globally installed binaries.
//!
//! [`scaffold_runweaver_project`] creates the directory structure
//! idempotently; [`install_managed_toolchain`] runs the package manager
//! install inside it; [`resolve_managed_binary`] locates a managed tool binary;
//! [`resolve_repo_local_binary`] locates host repo `node_modules/.bin` tools
//! before command execution falls back to `PATH`; [`managed_tool_path_env`]
//! builds the `PATH` value command tasks run with.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::diagnostics::{RunweaverDiagnostic, RunweaverDiagnosticsError, error_diagnostic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaffoldActionStatus {
    Created,
    Skipped,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaffoldAction {
    pub status: ScaffoldActionStatus,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedToolchainInstallResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ScaffoldError {
    #[error("Failed to update scaffold path `{}`: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveManagedBinaryResult {
    Found { path: PathBuf },
    Missing { diagnostic: RunweaverDiagnostic },
}

pub fn managed_toolchain_root(cwd: impl AsRef<Path>) -> PathBuf {
    cwd.as_ref().join(".runweaver")
}

pub fn scaffold_runweaver_project(
    cwd: impl AsRef<Path>,
) -> Result<Vec<ScaffoldAction>, ScaffoldError> {
    let cwd = cwd.as_ref();
    let mut actions = Vec::new();

    actions.extend(ensure_directory(cwd, ".runweaver")?);
    actions.extend(ensure_directory(cwd, ".runweaver/configs")?);
    actions.extend(ensure_file(
        cwd,
        ".runweaver/package.json",
        default_toolchain_package_json(),
    )?);
    actions.extend(ensure_gitignore_rule(cwd, ".runweaver/node_modules/")?);

    Ok(actions)
}

pub fn install_managed_toolchain(
    cwd: impl AsRef<Path>,
    executable: Option<&str>,
) -> Result<ManagedToolchainInstallResult, RunweaverDiagnosticsError> {
    let root = managed_toolchain_root(cwd);
    let package_json_path = root.join("package.json");
    if !package_json_path.exists() {
        return Err(RunweaverDiagnosticsError::new(
            "Runweaver toolchain package missing.",
            vec![
                error_diagnostic(
                    "RUNWEAVER_TOOLCHAIN_PACKAGE_MISSING",
                    ".runweaver/package.json is missing.",
                )
                .with_path(".runweaver/package.json"),
            ],
        ));
    }

    Ok(spawn_install(executable.unwrap_or("bun"), &root))
}

pub fn managed_bin_dir(cwd: impl AsRef<Path>) -> PathBuf {
    managed_toolchain_root(cwd)
        .join("node_modules")
        .join(".bin")
}

pub fn resolve_managed_binary(cwd: impl AsRef<Path>, program: &str) -> ResolveManagedBinaryResult {
    for candidate in binary_candidates(cwd.as_ref(), program) {
        if candidate.is_file() {
            return ResolveManagedBinaryResult::Found { path: candidate };
        }
    }

    ResolveManagedBinaryResult::Missing {
        diagnostic: error_diagnostic(
            "RUNWEAVER_BINARY_MISSING",
            format!(
                "Tool binary \"{program}\" was not found in .runweaver/node_modules/.bin or .runweaver/."
            ),
        )
        .with_path(format!(".runweaver/node_modules/.bin/{program}")),
    }
}

/// Resolves `program` from the nearest `node_modules/.bin` directory,
/// walking up from `cwd` to the filesystem root. Used after the managed
/// `.runweaver/` lookup and before falling back to `PATH`.
pub fn resolve_repo_local_binary(cwd: impl AsRef<Path>, program: &str) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        return None;
    }

    let names = binary_names(program);
    for ancestor in cwd.as_ref().ancestors() {
        for name in &names {
            let candidate = ancestor.join("node_modules").join(".bin").join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

pub fn managed_tool_path_env(cwd: impl AsRef<Path>, parent_path: Option<&str>) -> String {
    let mut entries = vec![
        path_to_string(&managed_bin_dir(&cwd)),
        path_to_string(&managed_toolchain_root(&cwd)),
    ];
    if let Some(parent_path) = parent_path.filter(|path| !path.is_empty()) {
        entries.push(parent_path.to_owned());
    }
    entries.join(if cfg!(windows) { ";" } else { ":" })
}

fn binary_candidates(cwd: &Path, program: &str) -> Vec<PathBuf> {
    let root = managed_toolchain_root(cwd);
    if program.contains('/') || program.contains('\\') {
        return vec![root.join(program)];
    }

    let names = binary_names(program);
    names
        .iter()
        .map(|name| root.join("node_modules").join(".bin").join(name))
        .chain(names.iter().map(|name| root.join(name)))
        .collect()
}

fn binary_names(program: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            format!("{program}.cmd"),
            format!("{program}.exe"),
            program.to_owned(),
        ]
    } else {
        vec![program.to_owned()]
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn spawn_install(executable: &str, cwd: &Path) -> ManagedToolchainInstallResult {
    match Command::new(executable)
        .arg("install")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => ManagedToolchainInstallResult {
            exit_code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        },
        Err(error) => ManagedToolchainInstallResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: format!("failed to spawn {executable}: {error}\n"),
        },
    }
}

fn ensure_directory(cwd: &Path, relative_path: &str) -> Result<Vec<ScaffoldAction>, ScaffoldError> {
    let target = cwd.join(relative_path);
    if target.exists() {
        return Ok(vec![ScaffoldAction {
            status: ScaffoldActionStatus::Skipped,
            path: relative_path.to_owned(),
            message: "already exists".to_owned(),
        }]);
    }

    std::fs::create_dir_all(&target).map_err(|source| ScaffoldError::Io {
        path: target,
        source,
    })?;
    Ok(vec![ScaffoldAction {
        status: ScaffoldActionStatus::Created,
        path: relative_path.to_owned(),
        message: "created directory".to_owned(),
    }])
}

fn ensure_file(
    cwd: &Path,
    relative_path: &str,
    contents: &str,
) -> Result<Vec<ScaffoldAction>, ScaffoldError> {
    let target = cwd.join(relative_path);
    if target.exists() {
        return Ok(vec![ScaffoldAction {
            status: ScaffoldActionStatus::Skipped,
            path: relative_path.to_owned(),
            message: "preserved existing file".to_owned(),
        }]);
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ScaffoldError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(&target, contents).map_err(|source| ScaffoldError::Io {
        path: target,
        source,
    })?;
    Ok(vec![ScaffoldAction {
        status: ScaffoldActionStatus::Created,
        path: relative_path.to_owned(),
        message: "created file".to_owned(),
    }])
}

fn ensure_gitignore_rule(cwd: &Path, rule: &str) -> Result<Vec<ScaffoldAction>, ScaffoldError> {
    let relative_path = ".gitignore";
    let target = cwd.join(relative_path);
    let current = if target.exists() {
        std::fs::read_to_string(&target).map_err(|source| ScaffoldError::Io {
            path: target.clone(),
            source,
        })?
    } else {
        String::new()
    };
    let lines = current.lines().map(str::trim);
    if lines.into_iter().any(|line| line == rule) {
        return Ok(vec![ScaffoldAction {
            status: ScaffoldActionStatus::Skipped,
            path: relative_path.to_owned(),
            message: format!("{rule} already ignored"),
        }]);
    }

    let prefix = if current.is_empty() || current.ends_with('\n') {
        current.as_str()
    } else {
        ""
    };
    let contents = if prefix.is_empty() && !current.is_empty() {
        format!("{current}\n{rule}\n")
    } else {
        format!("{prefix}{rule}\n")
    };
    let status = if current.is_empty() {
        ScaffoldActionStatus::Created
    } else {
        ScaffoldActionStatus::Changed
    };
    std::fs::write(&target, contents).map_err(|source| ScaffoldError::Io {
        path: target,
        source,
    })?;

    Ok(vec![ScaffoldAction {
        status,
        path: relative_path.to_owned(),
        message: format!("added {rule}"),
    }])
}

fn default_toolchain_package_json() -> &'static str {
    "{\n  \"private\": true,\n  \"type\": \"module\",\n  \"dependencies\": {}\n}\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_managed_binary_finds_project_local_bin() {
        let root = test_root("found");
        let bin_dir = managed_bin_dir(&root);
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("tool"), "").unwrap();

        let result = resolve_managed_binary(&root, "tool");

        assert!(matches!(result, ResolveManagedBinaryResult::Found { .. }));
    }

    #[test]
    fn resolve_repo_local_binary_finds_bin_in_cwd_node_modules() {
        let root = test_root("repo-local-found");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let tool = bin_dir.join("tool");
        std::fs::write(&tool, "").unwrap();

        let result = resolve_repo_local_binary(&root, "tool");

        assert_eq!(result, Some(tool));
    }

    #[test]
    fn resolve_repo_local_binary_walks_up_to_ancestor_node_modules() {
        let root = test_root("repo-local-ancestor");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let tool = bin_dir.join("tool");
        std::fs::write(&tool, "").unwrap();
        let app = root.join("packages").join("app");
        std::fs::create_dir_all(&app).unwrap();

        let result = resolve_repo_local_binary(&app, "tool");

        assert_eq!(result, Some(tool));
    }

    #[test]
    fn resolve_repo_local_binary_returns_none_when_absent() {
        let root = test_root("repo-local-absent");

        let result = resolve_repo_local_binary(&root, "tool");

        assert_eq!(result, None);
    }

    #[test]
    fn resolve_repo_local_binary_ignores_path_shaped_programs() {
        let root = test_root("repo-local-path-shaped");
        let bin_dir = root.join("node_modules").join(".bin").join("dir");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("tool"), "").unwrap();

        let result = resolve_repo_local_binary(&root, "dir/tool");

        assert_eq!(result, None);
    }

    #[test]
    fn scaffold_runweaver_project_creates_missing_scaffold_and_is_idempotent() {
        let root = test_root("scaffold");

        let first = scaffold_runweaver_project(&root).unwrap();
        let second = scaffold_runweaver_project(&root).unwrap();

        assert!(
            first
                .iter()
                .any(|action| action.status == ScaffoldActionStatus::Created)
        );
        assert!(
            second
                .iter()
                .all(|action| action.status == ScaffoldActionStatus::Skipped)
        );
        assert!(root.join(".runweaver/package.json").exists());
        assert!(root.join(".runweaver/configs").exists());
        assert!(
            std::fs::read_to_string(root.join(".gitignore"))
                .unwrap()
                .contains(".runweaver/node_modules/")
        );
    }

    #[test]
    fn scaffold_runweaver_project_preserves_authored_files() {
        let root = test_root("scaffold-authored");
        std::fs::create_dir_all(root.join(".runweaver")).unwrap();
        std::fs::write(
            root.join(".runweaver/package.json"),
            "{\"authored\":true}\n",
        )
        .unwrap();

        scaffold_runweaver_project(&root).unwrap();

        assert!(
            std::fs::read_to_string(root.join(".runweaver/package.json"))
                .unwrap()
                .contains("authored")
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_managed_toolchain_runs_package_manager_inside_toolchain_root() {
        let root = test_root("install");
        let toolchain_root = managed_toolchain_root(&root);
        std::fs::create_dir_all(&toolchain_root).unwrap();
        std::fs::write(toolchain_root.join("package.json"), "{\"private\":true}\n").unwrap();
        // Run the fake package manager as `sh install` instead of exec-ing a
        // freshly written executable: exec-ing it races with concurrent
        // fork/exec in parallel tests (ETXTBSY). `spawn_install` passes
        // `install` as the argument and the toolchain root as cwd, so `sh`
        // reads this script.
        std::fs::write(
            toolchain_root.join("install"),
            r#"printf "%s" "$PWD" > ran-cwd.txt
mkdir -p node_modules/.bin
touch bun.lock
"#,
        )
        .unwrap();

        let result = install_managed_toolchain(&root, Some("sh")).unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(toolchain_root.join("bun.lock").exists());
        assert!(toolchain_root.join("node_modules/.bin").exists());
        assert_eq!(
            std::fs::read_to_string(toolchain_root.join("ran-cwd.txt")).unwrap(),
            path_to_string(&toolchain_root)
        );
    }

    #[test]
    fn install_managed_toolchain_reports_missing_package_as_diagnostic_error() {
        let root = test_root("install-missing-package");

        let error = install_managed_toolchain(&root, Some("bun")).unwrap_err();

        assert_eq!(error.message, "Runweaver toolchain package missing.");
        assert_eq!(
            error.diagnostics[0].code,
            "RUNWEAVER_TOOLCHAIN_PACKAGE_MISSING"
        );
        assert_eq!(
            error.diagnostics[0].path.as_deref(),
            Some(".runweaver/package.json")
        );
    }

    #[test]
    fn install_managed_toolchain_reports_spawn_failure_as_install_result() {
        let root = test_root("install-spawn-failure");
        let toolchain_root = managed_toolchain_root(&root);
        std::fs::create_dir_all(&toolchain_root).unwrap();
        std::fs::write(toolchain_root.join("package.json"), "{\"private\":true}\n").unwrap();

        let result =
            install_managed_toolchain(&root, Some("runweaver-definitely-missing-executable"))
                .unwrap();

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert!(
            result
                .stderr
                .starts_with("failed to spawn runweaver-definitely-missing-executable:")
        );
    }

    #[test]
    fn resolve_managed_binary_reports_stable_missing_diagnostic() {
        let root = test_root("missing");

        let ResolveManagedBinaryResult::Missing { diagnostic } =
            resolve_managed_binary(&root, "missing-tool")
        else {
            panic!("expected missing diagnostic");
        };

        assert_eq!(diagnostic.code, "RUNWEAVER_BINARY_MISSING");
        assert_eq!(
            diagnostic.path.as_deref(),
            Some(".runweaver/node_modules/.bin/missing-tool")
        );
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "runweaver-toolchain-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
