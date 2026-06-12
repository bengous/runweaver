use anyhow::{Result, anyhow};
use serde_json::{Map, Value};

use super::contract::{
    HookEmission, HookEnv, HookEvent, HookOutcome, HookRequest, HookStage, UpdatedFileSnapshot,
};
use super::failure::hook_failure_reason;
use super::payload::{
    optional_bool, optional_string, parse_payload, require_event_name, require_object,
    require_present_field, require_string,
};
use super::tool_input::touched_path_candidates;
use crate::surfaces::agent_hooks::runtime::HarnessCodec;

/// Codec for the Codex hooks protocol (`.codex/config.toml` harness).
pub struct CodexCodec;

impl HarnessCodec for CodexCodec {
    fn harness(&self) -> &'static str {
        "codex"
    }

    fn decode(&self, stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
        decode_codex_hook(stdin, stage)
    }

    fn encode(&self, outcome: HookOutcome, request: &HookRequest) -> HookEmission {
        encode_codex_hook_outcome(outcome, request)
    }

    fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
        encode_json_hook_failure(stage, hook_failure_reason(stage, error))
    }
}

/// Codec for the Claude Code hooks protocol (`.claude/settings.json`
/// harness). Preserves tool-response shape when emitting
/// `updatedToolOutput` for post-edit rewrites.
pub struct ClaudeCodec;

impl HarnessCodec for ClaudeCodec {
    fn harness(&self) -> &'static str {
        "claude"
    }

    fn decode(&self, stdin: &str, stage: HookStage, _env: &HookEnv) -> Result<HookRequest> {
        decode_claude_hook(stdin, stage)
    }

    fn encode(&self, outcome: HookOutcome, request: &HookRequest) -> HookEmission {
        encode_claude_hook_outcome(outcome, request)
    }

    fn encode_failure(&self, stage: HookStage, error: &anyhow::Error) -> HookEmission {
        encode_json_hook_failure(stage, hook_failure_reason(stage, error))
    }
}

fn decode_codex_hook(stdin: &str, stage: HookStage) -> Result<HookRequest> {
    let value = parse_payload(stdin, "Codex")?;
    require_event_name(&value, "Codex", stage)?;

    let session_id = require_string(&value, "session_id", "Codex")?;
    require_string(&value, "turn_id", "Codex")?;
    let cwd = require_string(&value, "cwd", "Codex")?;
    let stop_hook_active = if stage == HookStage::Stop {
        optional_bool(&value, "stop_hook_active", "Codex")?
    } else {
        None
    };
    let tool_input = match stage {
        HookStage::Stop => Map::new(),
        HookStage::PreTool | HookStage::PostEdit => {
            require_object(&value, "tool_input", "Codex")?.clone()
        }
    };
    let tool_response = match stage {
        HookStage::PostEdit => Some(require_present_field(&value, "tool_response", "Codex")?),
        HookStage::PreTool | HookStage::Stop => None,
    };
    let tool_call_id = match stage {
        HookStage::Stop => None,
        HookStage::PreTool | HookStage::PostEdit => {
            Some(require_string(&value, "tool_use_id", "Codex")?)
        }
    };
    let tool_name = match stage {
        HookStage::Stop => None,
        HookStage::PreTool | HookStage::PostEdit => {
            Some(require_string(&value, "tool_name", "Codex")?)
        }
    };
    let tool_command = codex_command_from_tool_input(&tool_input, tool_name.as_deref())?;
    Ok(HookRequest {
        event: HookEvent {
            harness: "codex".to_owned(),
            stage,
            session_id,
            tool_call_id,
            transcript_path: None,
            cwd,
            touched_path_candidates: touched_path_candidates(&tool_input),
            patch_text: tool_command.clone(),
            tool_command,
            tool_name,
            tool_response,
            stop_hook_active: stop_hook_active.unwrap_or(false),
        },
    })
}

fn decode_claude_hook(stdin: &str, stage: HookStage) -> Result<HookRequest> {
    let value = parse_payload(stdin, "Claude")?;
    require_event_name(&value, "Claude", stage)?;

    let session_id = require_string(&value, "session_id", "Claude")?;
    let transcript_path = require_string(&value, "transcript_path", "Claude")?;
    let cwd = require_string(&value, "cwd", "Claude")?;
    let tool_call_id = match stage {
        HookStage::Stop => None,
        HookStage::PreTool | HookStage::PostEdit => {
            Some(require_string(&value, "tool_use_id", "Claude")?)
        }
    };
    let tool_name = match stage {
        HookStage::Stop => None,
        HookStage::PreTool | HookStage::PostEdit => {
            Some(require_string(&value, "tool_name", "Claude")?)
        }
    };
    let stop_hook_active = if stage == HookStage::Stop {
        optional_bool(&value, "stop_hook_active", "Claude")?
    } else {
        None
    };
    let tool_input = match stage {
        HookStage::Stop => Map::new(),
        HookStage::PreTool | HookStage::PostEdit => {
            require_object(&value, "tool_input", "Claude")?.clone()
        }
    };
    let tool_response = match stage {
        HookStage::PostEdit => Some(require_present_field(&value, "tool_response", "Claude")?),
        HookStage::PreTool | HookStage::Stop => None,
    };
    let tool_command = claude_command_from_tool_input(&tool_input, tool_name.as_deref())?;
    Ok(HookRequest {
        event: HookEvent {
            harness: "claude".to_owned(),
            stage,
            session_id,
            tool_call_id,
            transcript_path: Some(transcript_path),
            cwd,
            touched_path_candidates: touched_path_candidates(&tool_input),
            patch_text: tool_command.clone(),
            tool_command,
            tool_name,
            tool_response,
            stop_hook_active: stop_hook_active.unwrap_or(false),
        },
    })
}

fn codex_command_from_tool_input(
    tool_input: &Map<String, Value>,
    tool_name: Option<&str>,
) -> Result<Option<String>> {
    let command = optional_string(tool_input, "command", "Codex tool_input")?;
    if matches!(tool_name, Some("Bash" | "apply_patch")) && command.is_none() {
        return Err(anyhow!(
            "Codex hook payload field tool_input.command must be a non-empty string for Bash/apply_patch."
        ));
    }
    Ok(command)
}

fn claude_command_from_tool_input(
    tool_input: &Map<String, Value>,
    tool_name: Option<&str>,
) -> Result<Option<String>> {
    let command = optional_string(tool_input, "command", "Claude tool_input")?;
    if tool_name == Some("Bash") && command.is_none() {
        return Err(anyhow!(
            "Claude hook payload field tool_input.command must be a non-empty string for Bash."
        ));
    }
    Ok(command)
}

fn encode_codex_hook_outcome(outcome: HookOutcome, request: &HookRequest) -> HookEmission {
    match (request.event.stage, outcome) {
        (HookStage::PreTool, HookOutcome::Block { reason, .. }) => pre_tool_json_denial(reason),
        (HookStage::PreTool, HookOutcome::Pass { .. }) => HookEmission::pass(None),
        (HookStage::Stop, HookOutcome::Block { reason, .. }) => json_emission(serde_json::json!({
            "decision": "block",
            "reason": reason,
        })),
        (HookStage::Stop, HookOutcome::Pass { system_message, .. }) => {
            let mut payload =
                serde_json::Map::from_iter([("continue".to_owned(), serde_json::json!(true))]);
            if let Some(message) = system_message {
                payload.insert("systemMessage".to_owned(), serde_json::json!(message));
            }
            json_emission(serde_json::Value::Object(payload))
        }
        (HookStage::PostEdit, HookOutcome::Block { reason, .. }) => {
            json_emission(block_payload(reason))
        }
        (
            HookStage::PostEdit,
            HookOutcome::Pass {
                system_message: None,
                ..
            },
        ) => HookEmission::pass(None),
        (
            HookStage::PostEdit,
            HookOutcome::Pass {
                system_message: Some(message),
                ..
            },
        ) => post_tool_use_context_emission(message),
    }
}

fn encode_claude_hook_outcome(outcome: HookOutcome, request: &HookRequest) -> HookEmission {
    match request.event.stage {
        HookStage::PreTool => match outcome {
            HookOutcome::Block { reason, .. } => pre_tool_json_denial(reason),
            HookOutcome::Pass { .. } => HookEmission::pass(None),
        },
        HookStage::PostEdit => {
            let payload = claude_post_tool_use_payload(outcome, request);
            match payload {
                Some(payload) => json_emission(payload),
                None => HookEmission::pass(None),
            }
        }
        HookStage::Stop => {
            let payload = claude_payload(outcome);
            if payload.is_empty() {
                HookEmission::pass(None)
            } else {
                json_emission(serde_json::Value::Object(payload))
            }
        }
    }
}

fn claude_post_tool_use_payload(
    outcome: HookOutcome,
    request: &HookRequest,
) -> Option<serde_json::Value> {
    let updated_file = match &outcome {
        HookOutcome::Pass { updated_file, .. } | HookOutcome::Block { updated_file, .. } => {
            updated_file.as_ref()
        }
    };
    let updated_tool_output =
        updated_file.and_then(|file| claude_updated_tool_output(file, request));
    let mut payload = claude_payload(outcome);
    let has_hook_specific_output = payload
        .get("hookSpecificOutput")
        .is_some_and(serde_json::Value::is_object);

    if updated_tool_output.is_none() && !has_hook_specific_output {
        return if payload
            .get("decision")
            .is_some_and(|value| value == "block")
        {
            Some(serde_json::Value::Object(payload))
        } else {
            None
        };
    }

    let Some(updated_tool_output) = updated_tool_output else {
        return Some(serde_json::Value::Object(payload));
    };

    let mut hook_specific_output = payload
        .remove("hookSpecificOutput")
        .and_then(|value| match value {
            serde_json::Value::Object(output) => Some(output),
            _ => None,
        })
        .unwrap_or_default();
    hook_specific_output.insert("hookEventName".to_owned(), serde_json::json!("PostToolUse"));
    hook_specific_output.insert("updatedToolOutput".to_owned(), updated_tool_output);
    payload.insert(
        "hookSpecificOutput".to_owned(),
        serde_json::Value::Object(hook_specific_output),
    );
    Some(serde_json::Value::Object(payload))
}

fn claude_payload(outcome: HookOutcome) -> serde_json::Map<String, serde_json::Value> {
    let mut payload = serde_json::Map::new();
    match outcome {
        HookOutcome::Block {
            reason,
            system_message,
            ..
        } => {
            payload.insert("decision".to_owned(), serde_json::json!("block"));
            payload.insert("reason".to_owned(), serde_json::json!(reason));
            if let Some(message) = system_message {
                add_post_tool_use_context(&mut payload, message);
            }
        }
        HookOutcome::Pass { system_message, .. } => {
            if let Some(message) = system_message {
                add_post_tool_use_context(&mut payload, message);
            }
        }
    }
    payload
}

fn add_post_tool_use_context(
    payload: &mut serde_json::Map<String, serde_json::Value>,
    message: String,
) {
    payload.insert(
        "hookSpecificOutput".to_owned(),
        serde_json::json!({
            "hookEventName": "PostToolUse",
            "additionalContext": message,
        }),
    );
}

fn claude_updated_tool_output(
    updated_file: &UpdatedFileSnapshot,
    request: &HookRequest,
) -> Option<serde_json::Value> {
    let tool_name = request.event.tool_name.as_deref()?;
    let response = request.event.tool_response.as_ref()?.as_object()?;
    if !same_file(
        response.get("filePath")?,
        &updated_file.path,
        &request.event.cwd,
    ) {
        return None;
    }

    match tool_name {
        "Write" => {
            if !response
                .get("content")
                .is_some_and(serde_json::Value::is_string)
            {
                return None;
            }
            let mut output = response.clone();
            output.insert("content".to_owned(), serde_json::json!(updated_file.after));
            Some(serde_json::Value::Object(output))
        }
        "Edit" => {
            if !response
                .get("oldString")
                .is_some_and(serde_json::Value::is_string)
                || !response
                    .get("newString")
                    .is_some_and(serde_json::Value::is_string)
                || !response.contains_key("originalFile")
            {
                return None;
            }
            Some(serde_json::Value::Object(response.clone()))
        }
        "MultiEdit" => None,
        _ => None,
    }
}

fn same_file(value: &serde_json::Value, updated_path: &str, cwd: &str) -> bool {
    let Some(response_path) = value.as_str() else {
        return false;
    };
    let updated = normalize_hook_path(updated_path);
    let response = normalize_hook_path(response_path);
    if response == updated {
        return true;
    }
    if std::path::Path::new(updated_path).is_absolute() {
        return false;
    }
    let absolute_updated = std::path::Path::new(cwd).join(updated_path);
    response == normalize_hook_path(&absolute_updated.to_string_lossy())
}

fn normalize_hook_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn block_payload(reason: String) -> serde_json::Value {
    serde_json::json!({
        "decision": "block",
        "reason": reason,
    })
}

fn post_tool_use_context_emission(message: String) -> HookEmission {
    json_emission(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": message,
        },
    }))
}

fn encode_json_hook_failure(stage: HookStage, reason: String) -> HookEmission {
    match stage {
        HookStage::PreTool => pre_tool_json_denial(reason),
        HookStage::PostEdit | HookStage::Stop => json_emission(serde_json::json!({
            "decision": "block",
            "reason": reason,
        })),
    }
}

fn pre_tool_json_denial(reason: String) -> HookEmission {
    json_emission(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        },
    }))
}

fn json_emission(payload: serde_json::Value) -> HookEmission {
    HookEmission {
        exit_code: 0,
        stdout: Some(format!("{payload}\n")),
        stderr: None,
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use serde_json::json;

    use super::*;

    fn request_for(harness: &str, stage: HookStage) -> HookRequest {
        HookRequest {
            event: HookEvent {
                harness: harness.to_owned(),
                stage,
                session_id: "session-123".to_owned(),
                tool_call_id: None,
                transcript_path: None,
                cwd: "/repo".to_owned(),
                touched_path_candidates: Vec::new(),
                patch_text: None,
                tool_command: None,
                tool_name: None,
                tool_response: None,
                stop_hook_active: false,
            },
        }
    }

    fn json_stdout(emission: HookEmission) -> serde_json::Value {
        let stdout = emission.stdout.expect("emission should write JSON stdout");
        serde_json::from_str::<serde_json::Value>(&stdout).expect("stdout should be JSON")
    }

    #[test]
    fn claude_write_post_edit_emits_same_shape_updated_tool_output() {
        let request = HookRequest {
            event: HookEvent {
                harness: "claude".to_owned(),
                stage: HookStage::PostEdit,
                session_id: "session-123".to_owned(),
                tool_call_id: Some("tool-123".to_owned()),
                transcript_path: Some("/tmp/transcript.jsonl".to_owned()),
                cwd: "/repo".to_owned(),
                touched_path_candidates: vec!["src/index.ts".to_owned()],
                patch_text: None,
                tool_command: None,
                tool_name: Some("Write".to_owned()),
                tool_response: Some(json!({
                    "type": "update",
                    "filePath": "/repo/src/index.ts",
                    "content": "export const main=true;\n",
                    "structuredPatch": [],
                    "originalFile": "old",
                    "extra": true
                })),
                stop_hook_active: false,
            },
        };
        let outcome = HookOutcome::Pass {
            system_message: None,
            updated_file: Some(UpdatedFileSnapshot {
                path: "src/index.ts".to_owned(),
                before: "export const main=true;\n".to_owned(),
                after: "export const main = true;\n".to_owned(),
            }),
        };

        let emission = ClaudeCodec.encode(outcome, &request);

        assert_eq!(emission.exit_code, 0);
        let stdout = emission
            .stdout
            .expect("Claude updated tool output needs stdout");
        let payload = serde_json::from_str::<serde_json::Value>(&stdout).expect("stdout is json");
        assert_eq!(
            payload,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "updatedToolOutput": {
                        "type": "update",
                        "filePath": "/repo/src/index.ts",
                        "content": "export const main = true;\n",
                        "structuredPatch": [],
                        "originalFile": "old",
                        "extra": true
                    }
                }
            })
        );
    }

    #[test]
    fn claude_edit_post_edit_preserves_updated_tool_output() {
        let request = HookRequest {
            event: HookEvent {
                harness: "claude".to_owned(),
                stage: HookStage::PostEdit,
                session_id: "session-123".to_owned(),
                tool_call_id: Some("tool-123".to_owned()),
                transcript_path: Some("/tmp/transcript.jsonl".to_owned()),
                cwd: "/repo".to_owned(),
                touched_path_candidates: vec!["src/index.ts".to_owned()],
                patch_text: None,
                tool_command: None,
                tool_name: Some("Edit".to_owned()),
                tool_response: Some(json!({
                    "type": "update",
                    "filePath": "src/index.ts",
                    "oldString": "export const main=true;\n",
                    "newString": "export const main = true;\n",
                    "originalFile": "export const main=true;\n",
                    "structuredPatch": [{ "kind": "replace" }]
                })),
                stop_hook_active: false,
            },
        };
        let outcome = HookOutcome::Block {
            reason: "formatted edited file".to_owned(),
            system_message: Some("Formatter changed src/index.ts".to_owned()),
            updated_file: Some(UpdatedFileSnapshot {
                path: "src/index.ts".to_owned(),
                before: "export const main=true;\n".to_owned(),
                after: "export const main = true;\n".to_owned(),
            }),
        };

        let payload = json_stdout(ClaudeCodec.encode(outcome, &request));

        assert_eq!(
            payload,
            json!({
                "decision": "block",
                "reason": "formatted edited file",
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": "Formatter changed src/index.ts",
                    "updatedToolOutput": {
                        "type": "update",
                        "filePath": "src/index.ts",
                        "oldString": "export const main=true;\n",
                        "newString": "export const main = true;\n",
                        "originalFile": "export const main=true;\n",
                        "structuredPatch": [{ "kind": "replace" }]
                    }
                }
            })
        );
    }

    #[test]
    fn codex_post_edit_block_emits_native_decision_json() {
        let request = request_for("codex", HookStage::PostEdit);

        let payload = json_stdout(CodexCodec.encode(HookOutcome::block("blocked edit"), &request));

        assert_eq!(
            payload,
            json!({
                "decision": "block",
                "reason": "blocked edit"
            })
        );
    }

    #[test]
    fn codex_post_edit_pass_with_message_emits_additional_context() {
        let request = request_for("codex", HookStage::PostEdit);
        let outcome = HookOutcome::Pass {
            system_message: Some("Formatter changed src/lib.rs".to_owned()),
            updated_file: None,
        };

        let payload = json_stdout(CodexCodec.encode(outcome, &request));

        assert_eq!(
            payload,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": "Formatter changed src/lib.rs"
                }
            })
        );
    }

    #[test]
    fn codex_stop_pass_emits_continue_payload_with_optional_system_message() {
        let request = request_for("codex", HookStage::Stop);
        let outcome = HookOutcome::Pass {
            system_message: Some("Validation passed".to_owned()),
            updated_file: None,
        };

        let payload = json_stdout(CodexCodec.encode(outcome, &request));

        assert_eq!(
            payload,
            json!({
                "continue": true,
                "systemMessage": "Validation passed"
            })
        );
    }

    #[test]
    fn claude_pre_tool_block_emits_native_denial_json() {
        let request = request_for("claude", HookStage::PreTool);

        let payload =
            json_stdout(ClaudeCodec.encode(HookOutcome::block("blocked command"), &request));

        assert_eq!(
            payload,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "blocked command"
                }
            })
        );
    }

    #[test]
    fn claude_stop_block_emits_native_decision_json() {
        let request = request_for("claude", HookStage::Stop);

        let payload =
            json_stdout(ClaudeCodec.encode(HookOutcome::block("validation failed"), &request));

        assert_eq!(
            payload,
            json!({
                "decision": "block",
                "reason": "validation failed"
            })
        );
    }

    #[test]
    fn claude_post_edit_failure_emits_native_decision_json() {
        let emission = ClaudeCodec.encode_failure(HookStage::PostEdit, &anyhow!("bad payload"));

        assert_eq!(emission.exit_code, 0);
        let payload = json_stdout(emission);
        assert_eq!(
            payload,
            json!({
                "decision": "block",
                "reason": "Post-edit hook failed before validation could complete.\nError: bad payload\nCheck the hook configuration and project dependencies, then retry the agent action."
            })
        );
    }

    #[test]
    fn codex_pre_tool_failure_emits_native_denial_json() {
        let emission = CodexCodec.encode_failure(HookStage::PreTool, &anyhow!("bad payload"));

        assert_eq!(emission.exit_code, 0);
        let stdout = emission.stdout.expect("Codex failure needs stdout");
        let payload = serde_json::from_str::<serde_json::Value>(&stdout).expect("stdout is json");
        assert_eq!(
            payload,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "Pre-tool hook failed before validation could complete.\nError: bad payload\nCheck the hook configuration and project dependencies, then retry the agent action."
                }
            })
        );
    }

    #[test]
    fn codex_decodes_pre_tool_payload() {
        let request = CodexCodec
            .decode(
                &json!({
                    "hook_event_name": "PreToolUse",
                    "session_id": "session-123",
                    "turn_id": "turn-123",
                    "cwd": "/repo",
                    "tool_use_id": "tool-123",
                    "tool_name": "Bash",
                    "tool_input": { "command": "git status" }
                })
                .to_string(),
                HookStage::PreTool,
                &HookEnv::new(),
            )
            .expect("Codex PreToolUse payload should decode");

        assert_eq!(request.event.harness, "codex");
        assert_eq!(request.event.session_id, "session-123");
        assert_eq!(request.event.tool_call_id.as_deref(), Some("tool-123"));
        assert_eq!(request.event.tool_command.as_deref(), Some("git status"));
        assert_eq!(request.event.patch_text.as_deref(), Some("git status"));
    }

    #[test]
    fn codex_decodes_post_edit_payload() {
        let request = CodexCodec
            .decode(
                &json!({
                    "hook_event_name": "PostToolUse",
                    "session_id": "session-123",
                    "turn_id": "turn-123",
                    "cwd": "/repo",
                    "tool_use_id": "tool-123",
                    "tool_name": "Edit",
                    "tool_input": { "file_path": "src/main.rs" },
                    "tool_response": { "success": true }
                })
                .to_string(),
                HookStage::PostEdit,
                &HookEnv::new(),
            )
            .expect("Codex PostToolUse payload should decode");

        assert_eq!(request.event.stage, HookStage::PostEdit);
        assert_eq!(request.event.touched_path_candidates, vec!["src/main.rs"]);
        assert_eq!(
            request.event.tool_response,
            Some(json!({ "success": true }))
        );
    }

    #[test]
    fn codex_decodes_stop_payload() {
        let request = CodexCodec
            .decode(
                &json!({
                    "hook_event_name": "Stop",
                    "session_id": "session-123",
                    "turn_id": "turn-123",
                    "cwd": "/repo",
                    "stop_hook_active": true
                })
                .to_string(),
                HookStage::Stop,
                &HookEnv::new(),
            )
            .expect("Codex Stop payload should decode");

        assert_eq!(request.event.stage, HookStage::Stop);
        assert!(request.event.stop_hook_active);
        assert!(request.event.tool_call_id.is_none());
    }

    #[test]
    fn codex_missing_required_field_reports_contract_error() {
        let error = CodexCodec
            .decode(
                &json!({
                    "hook_event_name": "Stop",
                    "turn_id": "turn-123",
                    "cwd": "/repo"
                })
                .to_string(),
                HookStage::Stop,
                &HookEnv::new(),
            )
            .expect_err("missing Codex session_id should fail");

        assert!(
            error
                .to_string()
                .contains("Codex hook payload is missing required field session_id")
        );
    }

    #[test]
    fn claude_decodes_pre_tool_payload() {
        let request = ClaudeCodec
            .decode(
                &json!({
                    "hook_event_name": "PreToolUse",
                    "session_id": "session-123",
                    "transcript_path": "/tmp/transcript.jsonl",
                    "cwd": "/repo",
                    "tool_use_id": "tool-123",
                    "tool_name": "Bash",
                    "tool_input": { "command": "cargo test" }
                })
                .to_string(),
                HookStage::PreTool,
                &HookEnv::new(),
            )
            .expect("Claude PreToolUse payload should decode");

        assert_eq!(request.event.harness, "claude");
        assert_eq!(
            request.event.transcript_path.as_deref(),
            Some("/tmp/transcript.jsonl")
        );
        assert_eq!(request.event.tool_command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn claude_decodes_post_edit_payload() {
        let request = ClaudeCodec
            .decode(
                &json!({
                    "hook_event_name": "PostToolUse",
                    "session_id": "session-123",
                    "transcript_path": "/tmp/transcript.jsonl",
                    "cwd": "/repo",
                    "tool_use_id": "tool-123",
                    "tool_name": "Write",
                    "tool_input": { "file_path": "src/lib.rs" },
                    "tool_response": {
                        "filePath": "/repo/src/lib.rs",
                        "content": "pub fn main() {}\n"
                    }
                })
                .to_string(),
                HookStage::PostEdit,
                &HookEnv::new(),
            )
            .expect("Claude PostToolUse payload should decode");

        assert_eq!(request.event.stage, HookStage::PostEdit);
        assert_eq!(request.event.tool_name.as_deref(), Some("Write"));
        assert_eq!(
            request
                .event
                .tool_response
                .as_ref()
                .and_then(|value| value.get("filePath")),
            Some(&json!("/repo/src/lib.rs"))
        );
    }

    #[test]
    fn claude_decodes_stop_payload() {
        let request = ClaudeCodec
            .decode(
                &json!({
                    "hook_event_name": "Stop",
                    "session_id": "session-123",
                    "transcript_path": "/tmp/transcript.jsonl",
                    "cwd": "/repo",
                    "stop_hook_active": true
                })
                .to_string(),
                HookStage::Stop,
                &HookEnv::new(),
            )
            .expect("Claude Stop payload should decode");

        assert_eq!(request.event.stage, HookStage::Stop);
        assert!(request.event.stop_hook_active);
        assert!(request.event.tool_call_id.is_none());
    }

    #[test]
    fn claude_missing_required_field_reports_contract_error() {
        let error = ClaudeCodec
            .decode(
                &json!({
                    "hook_event_name": "Stop",
                    "session_id": "session-123",
                    "cwd": "/repo"
                })
                .to_string(),
                HookStage::Stop,
                &HookEnv::new(),
            )
            .expect_err("missing Claude transcript_path should fail");

        assert!(
            error
                .to_string()
                .contains("Claude hook payload is missing required field transcript_path")
        );
    }
}
