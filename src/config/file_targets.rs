use std::collections::HashSet;

use super::tasks::{ExecutionContext, PolicyVerdict};
use super::{PolicyDefinition, policy};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTargetsOptions {
    pub extensions: Vec<String>,
    pub files: Vec<String>,
    pub prefixes: Vec<String>,
}

impl FileTargetsOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_extensions(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.extensions = extensions.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_files(mut self, files: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.files = files.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_prefixes(mut self, prefixes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.prefixes = prefixes.into_iter().map(Into::into).collect();
        self
    }
}

pub fn file_targets(options: FileTargetsOptions) -> FileTargets {
    let extensions = options
        .extensions
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let files = options.files.iter().map(String::as_str).collect::<Vec<_>>();
    let prefixes = options
        .prefixes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    FileTargets::new(&extensions, &files, &prefixes)
}

/// File matcher over extensions, exact paths, and path prefixes; built with
/// [`file_targets`]. Paths are normalized before matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTargets {
    extensions: HashSet<String>,
    exact_files: HashSet<String>,
    prefixes: Vec<String>,
    has_path_constraints: bool,
}

impl FileTargets {
    pub fn new(extensions: &[&str], files: &[&str], prefixes: &[&str]) -> Self {
        let extensions = extensions
            .iter()
            .map(|extension| normalize_extension(extension))
            .collect();
        let exact_files = files
            .iter()
            .map(|file| normalize_file_path(file))
            .collect::<HashSet<_>>();
        let prefixes = prefixes
            .iter()
            .map(|prefix| normalize_prefix(prefix))
            .collect::<Vec<_>>();
        let has_path_constraints = !exact_files.is_empty() || !prefixes.is_empty();
        Self {
            extensions,
            exact_files,
            prefixes,
            has_path_constraints,
        }
    }

    pub fn matches(&self, file: &str) -> bool {
        let normalized = normalize_file_path(file);
        self.matches_normalized(&normalized)
    }

    pub fn filter(&self, input_files: &[String]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut matched = Vec::new();
        for file in input_files {
            let normalized = normalize_file_path(file);
            if self.matches_normalized(&normalized) && seen.insert(normalized.clone()) {
                matched.push(normalized);
            }
        }
        matched
    }

    pub fn has(&self, input_files: &[String]) -> bool {
        input_files.is_empty() || input_files.iter().any(|file| self.matches(file))
    }

    pub fn resolve(&self, input_files: &[String], fallback: &[&str]) -> Vec<String> {
        if input_files.is_empty() {
            fallback.iter().map(|file| (*file).to_owned()).collect()
        } else {
            self.filter(input_files)
        }
    }

    fn matches_normalized(&self, normalized: &str) -> bool {
        self.matches_extension(normalized)
            && (!self.has_path_constraints
                || self.exact_files.contains(normalized)
                || self
                    .prefixes
                    .iter()
                    .any(|prefix| normalized.starts_with(prefix)))
    }

    fn matches_extension(&self, file: &str) -> bool {
        self.extensions.is_empty() || self.extensions.contains(extension_of(file))
    }
}

/// What a file-target policy does when the context has no matching files:
/// run the task anyway, or skip it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyScope {
    Allow,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTargetPolicyOptions {
    pub skip_reason: Option<String>,
    pub empty_scope: EmptyScope,
}

impl FileTargetPolicyOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_skip_reason(mut self, reason: impl Into<String>) -> Self {
        self.skip_reason = Some(reason.into());
        self
    }

    pub fn with_empty_scope(mut self, empty_scope: EmptyScope) -> Self {
        self.empty_scope = empty_scope;
        self
    }
}

impl Default for FileTargetPolicyOptions {
    fn default() -> Self {
        Self {
            skip_reason: None,
            empty_scope: EmptyScope::Allow,
        }
    }
}

/// Builds the common "only run for matching files" policy: allows when the
/// context's files match `targets`, otherwise applies the configured
/// [`EmptyScope`] behavior.
pub fn file_target_policy(
    targets: FileTargets,
    options: FileTargetPolicyOptions,
) -> PolicyDefinition {
    policy(move |ctx| {
        file_target_verdict(
            &targets,
            ctx,
            options.empty_scope,
            options.skip_reason.as_deref(),
        )
    })
}

pub fn file_target_verdict(
    targets: &FileTargets,
    ctx: &ExecutionContext,
    empty_scope: EmptyScope,
    skip_reason: Option<&str>,
) -> PolicyVerdict {
    let has_target = if ctx.files.is_empty() {
        empty_scope == EmptyScope::Allow
    } else {
        targets.has(&ctx.files)
    };
    if has_target {
        PolicyVerdict::Allow
    } else {
        PolicyVerdict::Skip {
            reason: skip_reason.map(str::to_owned),
        }
    }
}

pub fn normalize_file_path(file: &str) -> String {
    file.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

fn normalize_extension(extension: &str) -> String {
    if extension.starts_with('.') {
        extension.to_owned()
    } else {
        format!(".{extension}")
    }
}

fn normalize_prefix(prefix: &str) -> String {
    let normalized = normalize_file_path(prefix);
    if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    }
}

fn extension_of(file: &str) -> &str {
    file.rfind('.').map(|index| &file[index..]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_targets_options_match_typescript_constructor_behavior() {
        let targets = file_targets(
            FileTargetsOptions::new()
                .with_extensions(["ts", ".js", ".mjs"])
                .with_files([
                    "runweaver.config.ts",
                    ".runweaver/configs/commitlint.config.js",
                ])
                .with_prefixes(["harness", "platform/", ".runweaver/project-specific/"]),
        );

        assert!(targets.matches("./runweaver.config.ts"));
        assert!(targets.matches("platform\\cli\\main.ts"));
        assert!(!targets.matches(".runweaver/configs/dependency-cruiser.cjs"));
        assert!(!targets.matches("docs/future-hooks.example.ts"));
        assert_eq!(
            targets.filter(&[
                "./platform/cli/main.ts".to_owned(),
                "platform\\cli\\main.ts".to_owned(),
                "docs/future-hooks.example.ts".to_owned(),
                ".runweaver/configs/dependency-cruiser.cjs".to_owned(),
                ".runweaver/configs/commitlint.config.js".to_owned(),
            ]),
            vec![
                "platform/cli/main.ts".to_owned(),
                ".runweaver/configs/commitlint.config.js".to_owned(),
            ]
        );
        assert_eq!(
            targets.resolve(&Vec::new(), &["harness/", "platform/"]),
            vec!["harness/".to_owned(), "platform/".to_owned()]
        );
    }

    #[test]
    fn file_targets_report_scoped_applicability() {
        let targets = file_targets(
            FileTargetsOptions::new()
                .with_extensions(["ts", ".js", ".mjs"])
                .with_files(["runweaver.config.ts"])
                .with_prefixes([".runweaver/project-specific/"]),
        );

        assert!(targets.has(&Vec::new()));
        assert!(!targets.has(&[
            "README.md".to_owned(),
            "docs/future-hooks.example.ts".to_owned(),
        ]));
        assert!(targets.has(&[
            "README.md".to_owned(),
            ".runweaver/project-specific/checks/path-checks/cli.ts".to_owned(),
        ]));
    }

    #[test]
    fn file_target_policy_builds_captured_policy_definition() {
        let targets = file_targets(
            FileTargetsOptions::new()
                .with_extensions(["ts"])
                .with_prefixes(["src"]),
        );
        let target_policy = file_target_policy(
            targets,
            FileTargetPolicyOptions::new().with_skip_reason("No targets."),
        );

        assert_eq!(
            (target_policy.evaluate)(&ExecutionContext::new("/repo")),
            PolicyVerdict::Allow
        );
        assert_eq!(
            (target_policy.evaluate)(
                &ExecutionContext::new("/repo")
                    .with_files(vec!["README.md".to_owned(), "src/index.ts".to_owned()])
            ),
            PolicyVerdict::Allow
        );
        assert_eq!(
            (target_policy.evaluate)(
                &ExecutionContext::new("/repo").with_files(vec!["README.md".to_owned()])
            ),
            PolicyVerdict::Skip {
                reason: Some("No targets.".to_owned())
            }
        );
    }

    #[test]
    fn file_target_policy_can_skip_empty_scopes() {
        let targets = file_targets(
            FileTargetsOptions::new()
                .with_extensions(["ts"])
                .with_prefixes(["src"]),
        );
        let target_policy = file_target_policy(
            targets,
            FileTargetPolicyOptions::new()
                .with_empty_scope(EmptyScope::Skip)
                .with_skip_reason("No scoped targets."),
        );

        assert_eq!(
            (target_policy.evaluate)(&ExecutionContext::new("/repo")),
            PolicyVerdict::Skip {
                reason: Some("No scoped targets.".to_owned())
            }
        );
    }
}
