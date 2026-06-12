use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{RunweaverBinaryManifest, RunweaverFingerprintError, create_runweaver_binary_manifest};

/// Cargo package/binary inputs for building a Rust-owned Runweaver project binary.
#[derive(Clone, Copy)]
pub struct CompileCargoRunweaverBinaryOptions<'a> {
    pub cwd: &'a Path,
    pub package: &'a str,
    pub binary_name: &'a str,
    pub out_path: &'a str,
    pub fingerprint_roots: &'a [String],
}

/// Result of compiling and copying a Cargo Runweaver project binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileCargoRunweaverBinaryResult {
    pub outfile: PathBuf,
    pub manifest: RunweaverBinaryManifest,
}

/// Errors produced while building or installing a Cargo Runweaver project binary.
#[derive(Debug, thiserror::Error)]
pub enum CompileCargoRunweaverBinaryError {
    #[error(transparent)]
    Fingerprint(#[from] RunweaverFingerprintError),
    #[error("Failed to run cargo build: {0}")]
    CargoSpawn(#[source] std::io::Error),
    #[error("Cargo build failed with exit code {exit_code}.\nstdout:\n{stdout}\nstderr:\n{stderr}")]
    CargoBuildFailed {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    #[error("Failed to copy compiled binary from `{}` to `{}`: {source}", source_path.display(), out_path.display())]
    Copy {
        source_path: PathBuf,
        out_path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to create output directory `{}`: {source}", path.display())]
    CreateOutputDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to mark compiled binary executable at `{}`: {source}", path.display())]
    SetExecutable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoBuildOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

/// Builds a release Cargo binary, copies it to `out_path`, and returns its source manifest.
/// Builds the project package with `cargo build --release`, copies the
/// binary to its install path, and returns it with a fresh
/// [`RunweaverBinaryManifest`].
pub fn compile_cargo_runweaver_binary(
    options: CompileCargoRunweaverBinaryOptions<'_>,
) -> Result<CompileCargoRunweaverBinaryResult, CompileCargoRunweaverBinaryError> {
    compile_cargo_runweaver_binary_with_runner(options, run_cargo_build)
}

fn compile_cargo_runweaver_binary_with_runner(
    options: CompileCargoRunweaverBinaryOptions<'_>,
    mut run_cargo: impl FnMut(&Path, &[String]) -> Result<CargoBuildOutput, std::io::Error>,
) -> Result<CompileCargoRunweaverBinaryResult, CompileCargoRunweaverBinaryError> {
    let manifest = create_runweaver_binary_manifest(options.cwd, options.fingerprint_roots)?;
    let build_args = cargo_build_args(options.package, options.binary_name);
    let build_output = run_cargo(options.cwd, &build_args)
        .map_err(CompileCargoRunweaverBinaryError::CargoSpawn)?;
    if build_output.exit_code != 0 {
        return Err(CompileCargoRunweaverBinaryError::CargoBuildFailed {
            exit_code: build_output.exit_code,
            stdout: build_output.stdout,
            stderr: build_output.stderr,
        });
    }

    let source_path = cargo_release_binary_path(options.cwd, options.binary_name);
    let outfile = options.cwd.join(options.out_path);
    copy_binary(&source_path, &outfile)?;

    Ok(CompileCargoRunweaverBinaryResult { outfile, manifest })
}

fn cargo_build_args(package: &str, binary_name: &str) -> Vec<String> {
    vec![
        "build".to_owned(),
        "--release".to_owned(),
        "-p".to_owned(),
        package.to_owned(),
        "--bin".to_owned(),
        binary_name.to_owned(),
    ]
}

fn run_cargo_build(cwd: &Path, args: &[String]) -> Result<CargoBuildOutput, std::io::Error> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    Ok(CargoBuildOutput {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn cargo_release_binary_path(cwd: &Path, binary_name: &str) -> PathBuf {
    cwd.join("target")
        .join("release")
        .join(executable_name(binary_name))
}

fn executable_name(binary_name: &str) -> String {
    if cfg!(windows) {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_owned()
    }
}

fn copy_binary(
    source_path: &Path,
    out_path: &Path,
) -> Result<(), CompileCargoRunweaverBinaryError> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| {
            CompileCargoRunweaverBinaryError::CreateOutputDirectory {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }

    std::fs::copy(source_path, out_path).map_err(|source| {
        CompileCargoRunweaverBinaryError::Copy {
            source_path: source_path.to_path_buf(),
            out_path: out_path.to_path_buf(),
            source,
        }
    })?;

    mark_executable(out_path)?;
    Ok(())
}

#[cfg(unix)]
fn mark_executable(path: &Path) -> Result<(), CompileCargoRunweaverBinaryError> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|source| {
        CompileCargoRunweaverBinaryError::SetExecutable {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn mark_executable(_path: &Path) -> Result<(), CompileCargoRunweaverBinaryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn compile_cargo_runweaver_binary_builds_release_binary_and_copies_outfile() {
        let root = temp_root("compile-ok");
        write_source_tree(&root);
        let calls = RefCell::new(Vec::<Vec<String>>::new());

        let result = compile_cargo_runweaver_binary_with_runner(
            CompileCargoRunweaverBinaryOptions {
                cwd: &root,
                package: "demo-rs",
                binary_name: "demo-rs",
                out_path: ".runweaver/bin/demo-rs",
                fingerprint_roots: &[
                    "Cargo.toml".to_owned(),
                    "Cargo.lock".to_owned(),
                    "crates/demo-rs/src".to_owned(),
                ],
            },
            |cwd, args| {
                calls.borrow_mut().push(args.to_vec());
                let binary = cargo_release_binary_path(cwd, "demo-rs");
                fs::create_dir_all(binary.parent().unwrap()).unwrap();
                fs::write(binary, "compiled").unwrap();
                Ok(CargoBuildOutput {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            },
        )
        .unwrap();

        assert_eq!(
            calls.into_inner(),
            vec![vec![
                "build".to_owned(),
                "--release".to_owned(),
                "-p".to_owned(),
                "demo-rs".to_owned(),
                "--bin".to_owned(),
                "demo-rs".to_owned(),
            ]]
        );
        assert_eq!(fs::read_to_string(&result.outfile).unwrap(), "compiled");
        assert_eq!(result.manifest.input_count, 3);
        assert!(
            result
                .manifest
                .inputs
                .iter()
                .any(|input| input.path == "crates/demo-rs/src/main.rs")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compile_cargo_runweaver_binary_reports_build_failure_without_copying() {
        let root = temp_root("compile-fail");
        write_source_tree(&root);

        let error = compile_cargo_runweaver_binary_with_runner(
            CompileCargoRunweaverBinaryOptions {
                cwd: &root,
                package: "demo-rs",
                binary_name: "demo-rs",
                out_path: ".runweaver/bin/demo-rs",
                fingerprint_roots: &["Cargo.toml".to_owned()],
            },
            |_cwd, _args| {
                Ok(CargoBuildOutput {
                    exit_code: 101,
                    stdout: "stdout".to_owned(),
                    stderr: "stderr".to_owned(),
                })
            },
        )
        .unwrap_err();

        assert!(matches!(
            error,
            CompileCargoRunweaverBinaryError::CargoBuildFailed { exit_code: 101, .. }
        ));
        assert!(!root.join(".runweaver/bin/demo-rs").exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn write_source_tree(root: &Path) {
        fs::create_dir_all(root.join("crates/demo-rs/src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("Cargo.lock"), "# lock\n").unwrap();
        fs::write(root.join("crates/demo-rs/src/main.rs"), "fn main() {}\n").unwrap();
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "runweaver-cargo-compile-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
