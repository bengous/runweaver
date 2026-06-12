use super::contract::HookEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookCommandInput {
    pub tool_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookTouchedPathsInput {
    pub harness: String,
    pub cwd: String,
    pub session_id: String,
    pub tool_call_id: String,
    pub patch_text: Option<String>,
    pub touched_path_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookStopSessionInput {
    pub harness: String,
    pub cwd: String,
    pub session_id: String,
    pub stop_hook_active: bool,
    pub touched_path_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct HookInputError {
    pub message: String,
}

impl HookInputError {
    fn missing(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn hook_command_input(event: &HookEvent) -> Result<HookCommandInput, HookInputError> {
    Ok(HookCommandInput {
        tool_command: required_string(
            event.tool_command.as_deref(),
            "Normalized hook event is missing toolCommand for command hook input.",
        )?
        .to_owned(),
    })
}

pub fn hook_tool_touched_paths_input(
    event: &HookEvent,
) -> Result<HookTouchedPathsInput, HookInputError> {
    Ok(HookTouchedPathsInput {
        harness: event.harness.clone(),
        cwd: event.cwd.clone(),
        session_id: event.session_id.clone(),
        tool_call_id: required_string(
            event.tool_call_id.as_deref(),
            "Normalized hook event is missing toolCallId for touched-path hook input.",
        )?
        .to_owned(),
        patch_text: event.patch_text.clone(),
        touched_path_candidates: event.touched_path_candidates.clone(),
    })
}

pub fn hook_stop_session_input(event: &HookEvent) -> HookStopSessionInput {
    HookStopSessionInput {
        harness: event.harness.clone(),
        cwd: event.cwd.clone(),
        session_id: event.session_id.clone(),
        stop_hook_active: event.stop_hook_active,
        touched_path_candidates: event.touched_path_candidates.clone(),
    }
}

fn required_string<'a>(value: Option<&'a str>, message: &str) -> Result<&'a str, HookInputError> {
    value.ok_or_else(|| HookInputError::missing(message))
}

#[cfg(test)]
mod tests {
    use super::super::contract::HookStage;
    use super::*;

    fn hook_event() -> HookEvent {
        HookEvent {
            harness: "fixture".to_owned(),
            stage: HookStage::PreTool,
            session_id: "session-1".to_owned(),
            tool_call_id: None,
            transcript_path: None,
            cwd: "/repo".to_owned(),
            touched_path_candidates: vec!["src/a.ts".to_owned()],
            patch_text: None,
            tool_command: None,
            tool_name: None,
            tool_response: None,
            stop_hook_active: false,
        }
    }

    #[test]
    fn hook_command_input_projects_command_text() {
        let event = HookEvent {
            tool_command: Some("rm -rf dist".to_owned()),
            ..hook_event()
        };

        let input = hook_command_input(&event).unwrap();
        let missing = hook_command_input(&hook_event()).unwrap_err();

        assert_eq!(
            input,
            HookCommandInput {
                tool_command: "rm -rf dist".to_owned()
            }
        );
        assert_eq!(
            missing.to_string(),
            "Normalized hook event is missing toolCommand for command hook input."
        );
    }

    #[test]
    fn hook_tool_touched_paths_input_preserves_patch_text() {
        let event = HookEvent {
            tool_call_id: Some("tool-1".to_owned()),
            patch_text: Some("diff --git a/src/a.ts b/src/a.ts".to_owned()),
            ..hook_event()
        };

        let input = hook_tool_touched_paths_input(&event).unwrap();
        let missing = hook_tool_touched_paths_input(&hook_event()).unwrap_err();

        assert_eq!(
            input,
            HookTouchedPathsInput {
                harness: "fixture".to_owned(),
                cwd: "/repo".to_owned(),
                session_id: "session-1".to_owned(),
                tool_call_id: "tool-1".to_owned(),
                patch_text: Some("diff --git a/src/a.ts b/src/a.ts".to_owned()),
                touched_path_candidates: vec!["src/a.ts".to_owned()],
            }
        );
        assert_eq!(
            missing.to_string(),
            "Normalized hook event is missing toolCallId for touched-path hook input."
        );
    }

    #[test]
    fn hook_stop_session_input_does_not_require_tool_fields() {
        let event = HookEvent {
            stage: HookStage::Stop,
            stop_hook_active: true,
            touched_path_candidates: vec!["README.md".to_owned()],
            ..hook_event()
        };

        let input = hook_stop_session_input(&event);

        assert_eq!(
            input,
            HookStopSessionInput {
                harness: "fixture".to_owned(),
                cwd: "/repo".to_owned(),
                session_id: "session-1".to_owned(),
                stop_hook_active: true,
                touched_path_candidates: vec!["README.md".to_owned()],
            }
        );
    }
}
