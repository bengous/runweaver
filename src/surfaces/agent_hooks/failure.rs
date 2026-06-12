use super::contract::HookStage;

pub fn hook_failure_reason(stage: HookStage, error: &anyhow::Error) -> String {
    let label = match stage {
        HookStage::PreTool => "Pre-tool",
        HookStage::PostEdit => "Post-edit",
        HookStage::Stop => "Stop",
    };
    let message = error.to_string();
    let hint = if message.starts_with("expected value") || message.contains("JSON") {
        "Check that the hook payload is valid JSON, then retry the agent action."
    } else {
        "Check the hook configuration and project dependencies, then retry the agent action."
    };
    format!("{label} hook failed before validation could complete.\nError: {message}\n{hint}")
}
