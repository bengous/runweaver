use super::codecs::{ClaudeCodec, CodexCodec};
use super::command_prefix::RunweaverHookCommandCwd;
use super::harness::{AgentsSurfaceDefaults, Harness, HarnessDefinition, define_harness};
use super::harness_hook_config::{claude_harness_hook_config, codex_harness_hook_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltInHarnessName {
    Codex,
    Claude,
}

impl BuiltInHarnessName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

static CODEX_CODEC: CodexCodec = CodexCodec;
static CLAUDE_CODEC: ClaudeCodec = ClaudeCodec;

pub fn codex_harness() -> Harness<'static> {
    define_harness(HarnessDefinition {
        id: BuiltInHarnessName::Codex.as_str().to_owned(),
        codec: &CODEX_CODEC,
        hook_config: codex_harness_hook_config(),
        agents_surface: AgentsSurfaceDefaults::new(RunweaverHookCommandCwd::GitRoot)
            .with_target_option("features", serde_json::json!({ "hooks": true }))
            .with_bash_tool_matcher("^Bash$")
            .with_edit_tool_matcher("^(apply_patch|Edit|Write|MultiEdit)$")
            .with_stop_status("Running Codex validation"),
    })
}

pub fn claude_harness() -> Harness<'static> {
    define_harness(HarnessDefinition {
        id: BuiltInHarnessName::Claude.as_str().to_owned(),
        codec: &CLAUDE_CODEC,
        hook_config: claude_harness_hook_config(),
        agents_surface: AgentsSurfaceDefaults::new(RunweaverHookCommandCwd::Env(
            "CLAUDE_PROJECT_DIR".to_owned(),
        ))
        .with_target_option(
            "worktreeSymlinkDirectories",
            serde_json::json!(["node_modules"]),
        )
        .with_edit_tool_matcher("Edit|Write|MultiEdit")
        .with_destructive_guard_status("Checking for destructive commands...")
        .with_post_edit_status("Formatting and linting...")
        .with_stop_status("Scope-aware validation...")
        .with_path_zone_status("Checking path zones..."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_harnesses_compose_codecs_and_hook_configs() {
        let codex = codex_harness();
        let claude = claude_harness();

        assert_eq!(codex.id, "codex");
        assert_eq!(codex.codec.harness(), "codex");
        assert_eq!(codex.hook_config.default_path, ".codex/config.toml");
        assert_eq!(claude.id, "claude");
        assert_eq!(claude.codec.harness(), "claude");
        assert_eq!(claude.hook_config.default_path, ".claude/settings.json");
    }
}
