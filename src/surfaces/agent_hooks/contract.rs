use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Environment variables visible to a hook invocation.
pub type HookEnv = BTreeMap<String, String>;

/// Lifecycle point at which a harness invokes a hook: before a tool runs,
/// after a file-modifying tool ran, or when the agent session is stopping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HookStage {
    PreTool,
    PostEdit,
    Stop,
}

impl HookStage {
    pub fn expected_pi_event_name(self) -> &'static str {
        match self {
            Self::PreTool => "PreToolUse",
            Self::PostEdit => "PostToolUse",
            Self::Stop => "Stop",
        }
    }

    pub fn pre_tool_block_exit_code(self) -> i32 {
        match self {
            Self::PreTool => 2,
            Self::PostEdit | Self::Stop => 1,
        }
    }
}

/// Harness-neutral view of one hook invocation, produced by a
/// [`HarnessCodec`](super::HarnessCodec) from the native payload.
/// `touched_path_candidates` lists files the triggering tool may have
/// modified; `stop_hook_active` is true when a stop hook re-entered while a
/// previous stop validation is still running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEvent {
    pub harness: String,
    pub stage: HookStage,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub touched_path_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop_hook_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRequest {
    pub event: HookEvent,
}

/// Before/after content of a file a hook rewrote, so the harness can show
/// or apply the updated content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdatedFileSnapshot {
    pub path: String,
    pub before: String,
    pub after: String,
}

/// A hook command's decision: allow the event ([`Pass`](Self::Pass)) or
/// block it with an agent-facing `reason` ([`Block`](Self::Block)). Either
/// way it may attach a `system_message` (extra context, not a block) and an
/// [`UpdatedFileSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum HookOutcome {
    Pass {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_file: Option<UpdatedFileSnapshot>,
    },
    Block {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_file: Option<UpdatedFileSnapshot>,
    },
}

impl HookOutcome {
    pub fn pass() -> Self {
        Self::Pass {
            system_message: None,
            updated_file: None,
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self::Block {
            reason: reason.into(),
            system_message: None,
            updated_file: None,
        }
    }
}

/// The harness-native projection of a [`HookOutcome`]: an exit code plus
/// stdout/stderr text. Pre-tool blocks use exit code 2; post-edit and stop
/// blocks use 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEmission {
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

impl HookEmission {
    pub fn pass(message: Option<String>) -> Self {
        Self {
            exit_code: 0,
            stdout: message.map(|value| format!("{value}\n")),
            stderr: None,
        }
    }

    pub fn block(stage: HookStage, reason: impl Into<String>) -> Self {
        Self {
            exit_code: stage.pre_tool_block_exit_code(),
            stdout: None,
            stderr: Some(format!("{}\n", reason.into())),
        }
    }

    pub fn write_to(
        self,
        stdout: &mut dyn std::io::Write,
        stderr: &mut dyn std::io::Write,
    ) -> std::io::Result<i32> {
        if let Some(text) = self.stdout {
            stdout.write_all(text.as_bytes())?;
        }
        if let Some(text) = self.stderr {
            stderr.write_all(text.as_bytes())?;
        }
        Ok(self.exit_code)
    }
}
