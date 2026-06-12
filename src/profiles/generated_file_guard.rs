use std::sync::Arc;

use regex::Regex;

const DEFAULT_REASON: &str = "generated or protected file";

pub type GeneratedFileGuardPredicate = Arc<dyn Fn(&str) -> bool + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFileGuardFileRule {
    pub path: String,
    pub reason: Option<String>,
}

impl GeneratedFileGuardFileRule {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFileGuardPrefixRule {
    pub prefix: String,
    pub reason: Option<String>,
}

impl GeneratedFileGuardPrefixRule {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedFileGuardPatternRule {
    pub pattern: Regex,
    pub reason: Option<String>,
}

impl GeneratedFileGuardPatternRule {
    pub fn new(pattern: Regex) -> Self {
        Self {
            pattern,
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

#[derive(Clone)]
pub struct GeneratedFileGuardPredicateRule {
    pub predicate: GeneratedFileGuardPredicate,
    pub reason: Option<String>,
}

impl GeneratedFileGuardPredicateRule {
    pub fn new(predicate: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self {
            predicate: Arc::new(predicate),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

impl std::fmt::Debug for GeneratedFileGuardPredicateRule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeneratedFileGuardPredicateRule")
            .field("predicate", &"<fn>")
            .field("reason", &self.reason)
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct GeneratedFileGuardOptions {
    pub files: Vec<GeneratedFileGuardFileRule>,
    pub prefixes: Vec<GeneratedFileGuardPrefixRule>,
    pub patterns: Vec<GeneratedFileGuardPatternRule>,
    pub predicates: Vec<GeneratedFileGuardPredicateRule>,
    pub reason: Option<String>,
}

impl GeneratedFileGuardOptions {
    pub fn with_file(mut self, path: impl Into<String>) -> Self {
        self.files.push(GeneratedFileGuardFileRule::new(path));
        self
    }

    pub fn with_file_rule(mut self, rule: GeneratedFileGuardFileRule) -> Self {
        self.files.push(rule);
        self
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefixes
            .push(GeneratedFileGuardPrefixRule::new(prefix));
        self
    }

    pub fn with_prefix_rule(mut self, rule: GeneratedFileGuardPrefixRule) -> Self {
        self.prefixes.push(rule);
        self
    }

    pub fn with_pattern(mut self, pattern: Regex) -> Self {
        self.patterns
            .push(GeneratedFileGuardPatternRule::new(pattern));
        self
    }

    pub fn with_pattern_rule(mut self, rule: GeneratedFileGuardPatternRule) -> Self {
        self.patterns.push(rule);
        self
    }

    pub fn with_predicate(
        mut self,
        predicate: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.predicates
            .push(GeneratedFileGuardPredicateRule::new(predicate));
        self
    }

    pub fn with_predicate_rule(mut self, rule: GeneratedFileGuardPredicateRule) -> Self {
        self.predicates.push(rule);
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// Compiled rule set blocking edits to generated or protected files,
/// matched by exact path, prefix, pattern, or predicate; built with
/// [`generated_file_guard`]. Per-rule reasons override the guard default.
#[derive(Debug, Clone)]
pub struct GeneratedFileGuard {
    files: Vec<NormalizedFileRule>,
    prefixes: Vec<NormalizedPrefixRule>,
    patterns: Vec<NormalizedPatternRule>,
    predicates: Vec<NormalizedPredicateRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedFileGuardResult {
    Allowed {
        path: String,
    },
    Blocked {
        path: String,
        reason: String,
        message: String,
    },
}

impl GeneratedFileGuardResult {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }

    pub fn path(&self) -> &str {
        match self {
            Self::Allowed { path } | Self::Blocked { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedFileRule {
    path: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct NormalizedPrefixRule {
    prefix: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct NormalizedPatternRule {
    pattern: Regex,
    reason: String,
}

#[derive(Clone)]
struct NormalizedPredicateRule {
    predicate: GeneratedFileGuardPredicate,
    reason: String,
}

impl std::fmt::Debug for NormalizedPredicateRule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NormalizedPredicateRule")
            .field("predicate", &"<fn>")
            .field("reason", &self.reason)
            .finish()
    }
}

pub fn generated_file_guard(options: GeneratedFileGuardOptions) -> GeneratedFileGuard {
    let fallback_reason = options.reason.unwrap_or_else(|| DEFAULT_REASON.to_owned());
    GeneratedFileGuard {
        files: options
            .files
            .into_iter()
            .map(|rule| NormalizedFileRule {
                path: normalize_path(&rule.path),
                reason: rule.reason.unwrap_or_else(|| fallback_reason.clone()),
            })
            .collect(),
        prefixes: options
            .prefixes
            .into_iter()
            .map(|rule| NormalizedPrefixRule {
                prefix: normalize_prefix(&rule.prefix),
                reason: rule.reason.unwrap_or_else(|| fallback_reason.clone()),
            })
            .collect(),
        patterns: options
            .patterns
            .into_iter()
            .map(|rule| NormalizedPatternRule {
                pattern: rule.pattern,
                reason: rule.reason.unwrap_or_else(|| fallback_reason.clone()),
            })
            .collect(),
        predicates: options
            .predicates
            .into_iter()
            .map(|rule| NormalizedPredicateRule {
                predicate: rule.predicate,
                reason: rule.reason.unwrap_or_else(|| fallback_reason.clone()),
            })
            .collect(),
    }
}

impl GeneratedFileGuard {
    pub fn check(&self, path: &str) -> GeneratedFileGuardResult {
        let normalized_path = normalize_path(path);

        for rule in &self.files {
            if normalized_path == rule.path {
                return blocked(normalized_path, &rule.reason);
            }
        }

        for rule in &self.prefixes {
            if normalized_path.starts_with(&rule.prefix) {
                return blocked(normalized_path, &rule.reason);
            }
        }

        for rule in &self.patterns {
            if rule.pattern.is_match(&normalized_path) {
                return blocked(normalized_path, &rule.reason);
            }
        }

        for rule in &self.predicates {
            if (rule.predicate)(&normalized_path) {
                return blocked(normalized_path, &rule.reason);
            }
        }

        GeneratedFileGuardResult::Allowed {
            path: normalized_path,
        }
    }
}

fn normalize_prefix(prefix: &str) -> String {
    let mut normalized = normalize_path(prefix);
    while normalized.ends_with('*') {
        normalized.pop();
    }
    normalized
}

fn normalize_path(path: &str) -> String {
    let mut normalized = path.replace('\\', "/");
    while normalized.starts_with("./") {
        normalized = normalized[2..].to_owned();
    }
    normalized
}

fn blocked(path: String, reason: &str) -> GeneratedFileGuardResult {
    GeneratedFileGuardResult::Blocked {
        message: format!("Blocked generated/protected file: {path} ({reason})"),
        path,
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_file_guard_blocks_exact_files_with_stable_message() {
        let guard = generated_file_guard(
            GeneratedFileGuardOptions::default().with_file("generated/api.ts"),
        );

        let result = guard.check("./generated\\api.ts");

        assert_eq!(
            result,
            GeneratedFileGuardResult::Blocked {
                path: "generated/api.ts".to_owned(),
                reason: "generated or protected file".to_owned(),
                message: "Blocked generated/protected file: generated/api.ts (generated or protected file)".to_owned(),
            }
        );
    }

    #[test]
    fn generated_file_guard_blocks_prefix_and_glob_like_prefix_matches() {
        let guard = generated_file_guard(
            GeneratedFileGuardOptions::default()
                .with_prefix("dist/**")
                .with_prefix("types/*"),
        );

        let dist = guard.check("dist/client/index.ts");
        let types = guard.check("types/generated.d.ts");

        assert!(!dist.allowed());
        assert_eq!(dist.path(), "dist/client/index.ts");
        assert!(!types.allowed());
        assert_eq!(types.path(), "types/generated.d.ts");
    }

    #[test]
    fn generated_file_guard_allows_paths_that_match_no_rules() {
        let guard = generated_file_guard(
            GeneratedFileGuardOptions::default()
                .with_file("generated/api.ts")
                .with_prefix("dist/**")
                .with_pattern(Regex::new(r"\.generated\.ts$").unwrap())
                .with_predicate(|path| path.ends_with(".snapshot.ts")),
        );

        assert_eq!(
            guard.check("src/source.ts"),
            GeneratedFileGuardResult::Allowed {
                path: "src/source.ts".to_owned(),
            }
        );
    }

    #[test]
    fn generated_file_guard_uses_custom_reasons_from_pattern_and_predicate_rules() {
        let guard = generated_file_guard(
            GeneratedFileGuardOptions::default()
                .with_reason("fallback reason")
                .with_pattern_rule(
                    GeneratedFileGuardPatternRule::new(Regex::new(r"\.generated\.ts$").unwrap())
                        .with_reason("generated from schema"),
                )
                .with_predicate_rule(
                    GeneratedFileGuardPredicateRule::new(|path| path.contains("/locked/"))
                        .with_reason("owned by another pipeline"),
                ),
        );

        assert_eq!(
            guard.check("src/client.generated.ts"),
            GeneratedFileGuardResult::Blocked {
                path: "src/client.generated.ts".to_owned(),
                reason: "generated from schema".to_owned(),
                message: "Blocked generated/protected file: src/client.generated.ts (generated from schema)".to_owned(),
            }
        );
        assert_eq!(
            guard.check("src/locked/manual.ts"),
            GeneratedFileGuardResult::Blocked {
                path: "src/locked/manual.ts".to_owned(),
                reason: "owned by another pipeline".to_owned(),
                message: "Blocked generated/protected file: src/locked/manual.ts (owned by another pipeline)".to_owned(),
            }
        );
    }
}
