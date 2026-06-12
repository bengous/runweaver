use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Format version of [`RunweaverBinaryManifest`]; bumped on breaking changes.
pub const RUNWEAVER_BINARY_MANIFEST_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunweaverBinaryManifestInput {
    pub path: String,
    pub size: u64,
    pub digest: String,
}

/// Records what went into a compiled project binary: source roots, the
/// collected inputs, and a deterministic fingerprint over them, so
/// `check binary` can detect a stale build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunweaverBinaryManifest {
    pub version: u8,
    pub fingerprint: String,
    pub source_roots: Vec<String>,
    pub input_count: usize,
    pub inputs: Vec<RunweaverBinaryManifestInput>,
    pub built_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RunweaverFingerprintError {
    #[error("Failed to read binary fingerprint path `{}`: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to format binary manifest timestamp: {0}")]
    TimeFormat(#[from] time::error::Format),
}

pub fn create_runweaver_binary_manifest(
    repo_root: impl AsRef<Path>,
    source_roots: &[String],
) -> Result<RunweaverBinaryManifest, RunweaverFingerprintError> {
    create_runweaver_binary_manifest_with_timestamp(
        repo_root,
        source_roots,
        OffsetDateTime::now_utc().format(&Rfc3339)?,
    )
}

pub fn read_runweaver_binary_manifest_inputs(
    repo_root: impl AsRef<Path>,
    source_roots: &[String],
) -> Result<Vec<RunweaverBinaryManifestInput>, RunweaverFingerprintError> {
    let repo_root = repo_root.as_ref();
    let mut files = BTreeSet::new();
    for root in unique_sorted_paths(source_roots) {
        collect_source_files(repo_root, &root, &mut files)?;
    }

    files
        .into_iter()
        .map(|relative_path| {
            let content_path = repo_root.join(&relative_path);
            let content =
                std::fs::read(&content_path).map_err(|source| RunweaverFingerprintError::Io {
                    path: content_path,
                    source,
                })?;
            Ok(RunweaverBinaryManifestInput {
                path: relative_path,
                size: content.len() as u64,
                digest: format!("sha256-{}", sha256_hex(&content)),
            })
        })
        .collect()
}

/// Deterministic digest over sorted inputs (path, size, content digest);
/// identical sources always produce the same fingerprint.
pub fn fingerprint_manifest_inputs(inputs: &[RunweaverBinaryManifestInput]) -> String {
    let mut hash = Sha256::new();
    for input in inputs {
        hash.update(input.path.as_bytes());
        hash.update([0]);
        hash.update(input.size.to_string().as_bytes());
        hash.update([0]);
        hash.update(input.digest.as_bytes());
        hash.update(b"\n");
    }
    format!("sha256-{}", hex_digest(hash.finalize().as_slice()))
}

fn create_runweaver_binary_manifest_with_timestamp(
    repo_root: impl AsRef<Path>,
    source_roots: &[String],
    built_at: String,
) -> Result<RunweaverBinaryManifest, RunweaverFingerprintError> {
    let source_roots = unique_sorted_paths(source_roots);
    let inputs = read_runweaver_binary_manifest_inputs(repo_root, &source_roots)?;
    Ok(RunweaverBinaryManifest {
        version: RUNWEAVER_BINARY_MANIFEST_VERSION,
        fingerprint: fingerprint_manifest_inputs(&inputs),
        source_roots,
        input_count: inputs.len(),
        inputs,
        built_at,
    })
}

fn collect_source_files(
    repo_root: &Path,
    relative_path: &str,
    files: &mut BTreeSet<String>,
) -> Result<(), RunweaverFingerprintError> {
    let relative_path = normalize_relative_path(relative_path);
    if relative_path.is_empty() {
        return Ok(());
    }
    let absolute_path = repo_root.join(&relative_path);
    let metadata = match std::fs::symlink_metadata(&absolute_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(RunweaverFingerprintError::Io {
                path: absolute_path,
                source,
            });
        }
    };

    if metadata.is_dir() {
        let entries =
            std::fs::read_dir(&absolute_path).map_err(|source| RunweaverFingerprintError::Io {
                path: absolute_path.clone(),
                source,
            })?;
        for entry in entries {
            let entry = entry.map_err(|source| RunweaverFingerprintError::Io {
                path: absolute_path.clone(),
                source,
            })?;
            let entry_name = entry.file_name();
            let nested = Path::new(&relative_path).join(entry_name);
            collect_source_files(repo_root, &path_to_string(&nested), files)?;
        }
        return Ok(());
    }

    if metadata.is_file() && is_source_file(&relative_path) {
        files.insert(relative_path);
    }

    Ok(())
}

fn unique_sorted_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|path| normalize_relative_path(path))
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_source_file(file_path: &str) -> bool {
    if file_path.ends_with(".test.ts") || file_path.ends_with(".test.tsx") {
        return false;
    }
    matches!(
        Path::new(file_path)
            .extension()
            .and_then(|value| value.to_str()),
        Some("cjs" | "js" | "json" | "jsonc" | "lock" | "mjs" | "rs" | "toml" | "ts" | "tsx")
    )
}

fn normalize_relative_path(file_path: &str) -> String {
    file_path
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

fn sha256_hex(content: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(content);
    hex_digest(hash.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_runweaver_binary_manifest_inputs_collects_sorted_source_files() {
        let root = test_root("inputs");
        std::fs::create_dir_all(root.join("src/nested")).unwrap();
        std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
        std::fs::write(root.join("src/index.test.ts"), "test('skip', () => {})\n").unwrap();
        std::fs::write(root.join("src/nested/config.json"), "{\"ok\":true}\n").unwrap();
        std::fs::write(root.join("src/nested/notes.md"), "# ignored\n").unwrap();

        let inputs = read_runweaver_binary_manifest_inputs(
            &root,
            &[
                "./src".to_owned(),
                "src/nested".to_owned(),
                "missing".to_owned(),
            ],
        )
        .unwrap();

        assert_eq!(
            inputs
                .iter()
                .map(|input| input.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/index.ts", "src/nested/config.json"]
        );
        assert_eq!(inputs[0].size, 20);
        assert!(inputs[0].digest.starts_with("sha256-"));
    }

    #[test]
    fn fingerprint_manifest_inputs_hashes_path_size_and_digest_metadata() {
        let fingerprint = fingerprint_manifest_inputs(&[RunweaverBinaryManifestInput {
            path: "src/index.ts".to_owned(),
            size: 20,
            digest: "sha256-abc".to_owned(),
        }]);

        assert_eq!(
            fingerprint,
            "sha256-ca5673006e703031d5575dbdf33722095056ef9918bc0bff251ee65efa4f6806"
        );
    }

    #[test]
    fn create_runweaver_binary_manifest_preserves_normalized_roots_and_counts_inputs() {
        let root = test_root("manifest");
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(root.join("runweaver.config.ts"), "export default {};\n").unwrap();
        std::fs::write(root.join("config/settings.jsonc"), "{}\n").unwrap();

        let manifest = create_runweaver_binary_manifest_with_timestamp(
            &root,
            &["./config".to_owned(), "runweaver.config.ts".to_owned()],
            "2026-06-09T00:00:00Z".to_owned(),
        )
        .unwrap();

        assert_eq!(manifest.version, 1);
        assert_eq!(
            manifest.source_roots,
            vec!["config".to_owned(), "runweaver.config.ts".to_owned()]
        );
        assert_eq!(manifest.input_count, 2);
        assert_eq!(manifest.inputs.len(), 2);
        assert_eq!(manifest.built_at, "2026-06-09T00:00:00Z");
        assert!(manifest.fingerprint.starts_with("sha256-"));
    }

    #[test]
    fn manifest_serializes_with_public_camel_case_fields() {
        let manifest = RunweaverBinaryManifest {
            version: RUNWEAVER_BINARY_MANIFEST_VERSION,
            fingerprint: "sha256-fingerprint".to_owned(),
            source_roots: vec!["runweaver.config.ts".to_owned()],
            input_count: 1,
            inputs: vec![RunweaverBinaryManifestInput {
                path: "runweaver.config.ts".to_owned(),
                size: 18,
                digest: "sha256-input".to_owned(),
            }],
            built_at: "2026-06-09T00:00:00Z".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(manifest).unwrap(),
            serde_json::json!({
                "version": 1,
                "fingerprint": "sha256-fingerprint",
                "sourceRoots": ["runweaver.config.ts"],
                "inputCount": 1,
                "inputs": [
                    {
                        "path": "runweaver.config.ts",
                        "size": 18,
                        "digest": "sha256-input"
                    }
                ],
                "builtAt": "2026-06-09T00:00:00Z"
            })
        );
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "runweaver-fingerprint-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
