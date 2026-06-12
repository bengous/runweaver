use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use super::contract::{HookEvent, HookOutcome, HookStage, UpdatedFileSnapshot};

pub type HookFn = Arc<dyn Fn(&HookEvent) -> Result<HookOutcome> + Send + Sync + 'static>;
pub type HookProcedure = dyn Fn(&HookEvent) -> Result<HookOutcome> + Send + Sync + 'static;
pub type RuntimeServicesFactory<Services> = dyn Fn() -> Result<Services> + Send + Sync + 'static;
pub type RuntimeServicesRunner<Input, Services, CommandResult> =
    dyn Fn(Input, Services) -> Result<CommandResult> + Send + Sync + 'static;

/// A named hook command: a Rust closure registered for one [`HookStage`],
/// optionally restricted to specific harnesses, returning a
/// [`HookOutcome`].
#[derive(Clone)]
pub struct HookCommandSpec {
    name: String,
    stage: HookStage,
    harnesses: Option<Vec<String>>,
    run: HookFn,
}

impl HookCommandSpec {
    pub fn new(
        name: impl Into<String>,
        stage: HookStage,
        run: impl Fn(&HookEvent) -> Result<HookOutcome> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            stage,
            harnesses: None,
            run: Arc::new(run),
        }
    }

    pub fn with_harnesses(
        mut self,
        harnesses: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.harnesses = Some(harnesses.into_iter().map(Into::into).collect());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn stage(&self) -> HookStage {
        self.stage
    }

    pub fn harnesses(&self) -> Option<&[String]> {
        self.harnesses.as_deref()
    }

    pub fn run(&self, event: &HookEvent) -> Result<HookOutcome> {
        (self.run)(event)
    }

    pub fn is_enabled_for(&self, harness: &str) -> bool {
        self.harnesses
            .as_ref()
            .map(|harnesses| harnesses.iter().any(|candidate| candidate == harness))
            .unwrap_or(true)
    }
}

impl fmt::Debug for HookCommandSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HookCommandSpec")
            .field("name", &self.name)
            .field("stage", &self.stage)
            .field("harnesses", &self.harnesses)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookCommandError {
    #[error("Duplicate hook command: {command}")]
    DuplicateCommand { command: String },
    #[error("Unknown hook command: {command}")]
    UnknownCommand { command: String },
    #[error("Hook command is not enabled for {harness}: {command}")]
    DisabledForHarness { command: String, harness: String },
    #[error("{message}")]
    MissingField { message: String },
}

#[derive(Debug)]
pub struct HookCommandCatalog<'a> {
    commands: &'a [HookCommandSpec],
    names: Vec<&'a str>,
}

impl<'a> HookCommandCatalog<'a> {
    pub fn names(&self) -> &[&'a str] {
        &self.names
    }

    pub fn has(&self, command: &str) -> bool {
        self.commands.iter().any(|spec| spec.name() == command)
    }

    pub fn get(
        &self,
        command: &str,
        harness: &str,
    ) -> std::result::Result<&'a HookCommandSpec, HookCommandError> {
        let spec = self
            .commands
            .iter()
            .find(|spec| spec.name() == command)
            .ok_or_else(|| HookCommandError::UnknownCommand {
                command: command.to_owned(),
            })?;
        if !spec.is_enabled_for(harness) {
            return Err(HookCommandError::DisabledForHarness {
                command: command.to_owned(),
                harness: harness.to_owned(),
            });
        }
        Ok(spec)
    }
}

pub fn create_hook_command_catalog(
    commands: &[HookCommandSpec],
) -> std::result::Result<HookCommandCatalog<'_>, HookCommandError> {
    let mut seen = HashSet::new();
    for command in commands {
        if !seen.insert(command.name()) {
            return Err(HookCommandError::DuplicateCommand {
                command: command.name().to_owned(),
            });
        }
    }

    let mut names = commands
        .iter()
        .map(HookCommandSpec::name)
        .collect::<Vec<_>>();
    names.sort_unstable();

    Ok(HookCommandCatalog { commands, names })
}

pub struct ProjectedHookCommandOptions<InputFn, RunFn, OutcomeFn> {
    pub name: String,
    pub stage: HookStage,
    pub harnesses: Option<Vec<String>>,
    pub input: InputFn,
    pub run: RunFn,
    pub outcome: OutcomeFn,
}

impl<InputFn, RunFn, OutcomeFn> ProjectedHookCommandOptions<InputFn, RunFn, OutcomeFn> {
    pub fn new(
        name: impl Into<String>,
        stage: HookStage,
        input: InputFn,
        run: RunFn,
        outcome: OutcomeFn,
    ) -> Self {
        Self {
            name: name.into(),
            stage,
            harnesses: None,
            input,
            run,
            outcome,
        }
    }

    pub fn with_harnesses(
        mut self,
        harnesses: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.harnesses = Some(harnesses.into_iter().map(Into::into).collect());
        self
    }
}

pub fn define_hook_command<Input, CommandResult, InputFn, RunFn, OutcomeFn>(
    options: ProjectedHookCommandOptions<InputFn, RunFn, OutcomeFn>,
) -> HookCommandSpec
where
    Input: 'static,
    CommandResult: 'static,
    InputFn: Fn(&HookEvent) -> Result<Input> + Send + Sync + 'static,
    RunFn: Fn(Input) -> Result<CommandResult> + Send + Sync + 'static,
    OutcomeFn: Fn(CommandResult) -> HookOutcome + Send + Sync + 'static,
{
    let ProjectedHookCommandOptions {
        name,
        stage,
        harnesses,
        input,
        run,
        outcome,
    } = options;
    let spec = HookCommandSpec::new(name, stage, move |event| {
        let input = input(event)?;
        let result = run(input)?;
        Ok(outcome(result))
    });

    match harnesses {
        Some(harnesses) => spec.with_harnesses(harnesses),
        None => spec,
    }
}

pub fn with_runtime_services<Input, Services, CommandResult, Factory, Runner>(
    services: Factory,
    run: Runner,
) -> impl Fn(Input) -> Result<CommandResult> + Send + Sync + 'static
where
    Factory: Fn() -> Result<Services> + Send + Sync + 'static,
    Runner: Fn(Input, Services) -> Result<CommandResult> + Send + Sync + 'static,
{
    move |input| run(input, services()?)
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookFeedbackOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_file: Option<UpdatedFileSnapshot>,
}

pub fn is_hook_feedback_outcome(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    for key in object.keys() {
        if key != "blockReason" && key != "systemMessage" && key != "updatedFile" {
            return false;
        }
    }

    object.get("blockReason").is_none_or(Value::is_string)
        && object.get("systemMessage").is_none_or(Value::is_string)
        && object
            .get("updatedFile")
            .is_none_or(is_updated_file_snapshot)
}

pub fn block_reason_outcome(reason: Option<String>) -> HookOutcome {
    match reason {
        Some(reason) => HookOutcome::block(reason),
        None => HookOutcome::pass(),
    }
}

pub fn feedback_outcome(result: HookFeedbackOutcome) -> HookOutcome {
    match result.block_reason {
        Some(reason) => HookOutcome::Block {
            reason,
            system_message: result.system_message,
            updated_file: result.updated_file,
        },
        None => HookOutcome::Pass {
            system_message: result.system_message,
            updated_file: result.updated_file,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEventField {
    Harness,
    Stage,
    SessionId,
    Cwd,
    ToolCallId,
    TranscriptPath,
    StopHookActive,
    TouchedPathCandidates,
    PatchText,
    ToolCommand,
    ToolName,
    ToolResponse,
}

impl HookEventField {
    pub fn name(self) -> &'static str {
        match self {
            Self::Harness => "harness",
            Self::Stage => "stage",
            Self::SessionId => "sessionId",
            Self::Cwd => "cwd",
            Self::ToolCallId => "toolCallId",
            Self::TranscriptPath => "transcriptPath",
            Self::StopHookActive => "stopHookActive",
            Self::TouchedPathCandidates => "touchedPathCandidates",
            Self::PatchText => "patchText",
            Self::ToolCommand => "toolCommand",
            Self::ToolName => "toolName",
            Self::ToolResponse => "toolResponse",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookEventFieldValue<'a> {
    String(&'a str),
    Stage(HookStage),
    Bool(bool),
    StringSlice(&'a [String]),
    Json(&'a Value),
}

pub fn require_hook_event_field<'a>(
    event: &'a HookEvent,
    field: HookEventField,
    message: impl Into<String>,
) -> std::result::Result<HookEventFieldValue<'a>, HookCommandError> {
    let missing = || HookCommandError::MissingField {
        message: message.into(),
    };

    match field {
        HookEventField::Harness => Ok(HookEventFieldValue::String(&event.harness)),
        HookEventField::Stage => Ok(HookEventFieldValue::Stage(event.stage)),
        HookEventField::SessionId => Ok(HookEventFieldValue::String(&event.session_id)),
        HookEventField::Cwd => Ok(HookEventFieldValue::String(&event.cwd)),
        HookEventField::ToolCallId => event
            .tool_call_id
            .as_deref()
            .map(HookEventFieldValue::String)
            .ok_or_else(missing),
        HookEventField::TranscriptPath => event
            .transcript_path
            .as_deref()
            .map(HookEventFieldValue::String)
            .ok_or_else(missing),
        HookEventField::StopHookActive => Ok(HookEventFieldValue::Bool(event.stop_hook_active)),
        HookEventField::TouchedPathCandidates => Ok(HookEventFieldValue::StringSlice(
            &event.touched_path_candidates,
        )),
        HookEventField::PatchText => event
            .patch_text
            .as_deref()
            .map(HookEventFieldValue::String)
            .ok_or_else(missing),
        HookEventField::ToolCommand => event
            .tool_command
            .as_deref()
            .map(HookEventFieldValue::String)
            .ok_or_else(missing),
        HookEventField::ToolName => event
            .tool_name
            .as_deref()
            .map(HookEventFieldValue::String)
            .ok_or_else(missing),
        HookEventField::ToolResponse => event
            .tool_response
            .as_ref()
            .map(HookEventFieldValue::Json)
            .ok_or_else(missing),
    }
}

fn is_updated_file_snapshot(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("path").is_some_and(Value::is_string)
        && object.get("before").is_some_and(Value::is_string)
        && object.get("after").is_some_and(Value::is_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn hook_event() -> HookEvent {
        HookEvent {
            harness: "fixture".to_owned(),
            stage: HookStage::PreTool,
            session_id: "session".to_owned(),
            tool_call_id: None,
            transcript_path: None,
            cwd: "/repo".to_owned(),
            touched_path_candidates: Vec::new(),
            patch_text: None,
            tool_command: None,
            tool_name: None,
            tool_response: None,
            stop_hook_active: false,
        }
    }

    #[test]
    fn catalog_sorts_names_and_checks_harness_access() {
        let commands = vec![
            HookCommandSpec::new("zeta", HookStage::PreTool, |_| Ok(HookOutcome::pass())),
            HookCommandSpec::new("alpha", HookStage::PreTool, |_| Ok(HookOutcome::pass()))
                .with_harnesses(["codex"]),
        ];
        let catalog = create_hook_command_catalog(&commands).unwrap();

        assert_eq!(catalog.names(), &["alpha", "zeta"]);
        assert!(catalog.has("alpha"));
        assert_eq!(catalog.get("alpha", "codex").unwrap().name(), "alpha");
        assert_eq!(
            catalog.get("alpha", "claude").unwrap_err().to_string(),
            "Hook command is not enabled for claude: alpha"
        );
        assert_eq!(
            catalog.get("missing", "codex").unwrap_err().to_string(),
            "Unknown hook command: missing"
        );
    }

    #[test]
    fn catalog_rejects_duplicate_command_names() {
        let commands = vec![
            HookCommandSpec::new("guard", HookStage::PreTool, |_| Ok(HookOutcome::pass())),
            HookCommandSpec::new("guard", HookStage::Stop, |_| Ok(HookOutcome::pass())),
        ];

        assert_eq!(
            create_hook_command_catalog(&commands)
                .unwrap_err()
                .to_string(),
            "Duplicate hook command: guard"
        );
    }

    #[test]
    fn feedback_validator_matches_compact_feedback_shape() {
        assert!(is_hook_feedback_outcome(&json!({})));
        assert!(is_hook_feedback_outcome(&json!({
            "blockReason": "blocked",
            "systemMessage": "note",
            "updatedFile": {
                "path": "src/a.ts",
                "before": "old",
                "after": "new",
                "extra": true
            }
        })));

        assert!(!is_hook_feedback_outcome(&Value::Null));
        assert!(!is_hook_feedback_outcome(&json!([])));
        assert!(!is_hook_feedback_outcome(&json!({ "reason": "blocked" })));
        assert!(!is_hook_feedback_outcome(&json!({ "blockReason": 1 })));
        assert!(!is_hook_feedback_outcome(&json!({
            "updatedFile": { "path": "src/a.ts", "before": "old" }
        })));
    }

    #[test]
    fn feedback_helpers_convert_to_hook_outcomes() {
        let updated_file = UpdatedFileSnapshot {
            path: "src/a.ts".to_owned(),
            before: "old".to_owned(),
            after: "new".to_owned(),
        };

        assert_eq!(block_reason_outcome(None), HookOutcome::pass());
        assert_eq!(
            block_reason_outcome(Some("blocked".to_owned())),
            HookOutcome::block("blocked")
        );
        assert_eq!(
            feedback_outcome(HookFeedbackOutcome {
                block_reason: Some("blocked".to_owned()),
                system_message: Some("note".to_owned()),
                updated_file: Some(updated_file.clone()),
            }),
            HookOutcome::Block {
                reason: "blocked".to_owned(),
                system_message: Some("note".to_owned()),
                updated_file: Some(updated_file.clone()),
            }
        );
        assert_eq!(
            feedback_outcome(HookFeedbackOutcome {
                block_reason: None,
                system_message: Some("note".to_owned()),
                updated_file: Some(updated_file.clone()),
            }),
            HookOutcome::Pass {
                system_message: Some("note".to_owned()),
                updated_file: Some(updated_file),
            }
        );
    }

    #[test]
    fn projected_command_runs_input_handler_and_outcome_conversion() {
        let command = define_hook_command(ProjectedHookCommandOptions::new(
            "guard-example",
            HookStage::PreTool,
            |event: &HookEvent| match require_hook_event_field(
                event,
                HookEventField::ToolCommand,
                "missing tool command",
            )? {
                HookEventFieldValue::String(command) => Ok(command.to_owned()),
                _ => Err(anyhow!("toolCommand had unexpected shape")),
            },
            |command: String| {
                Ok(if command.contains("rm -rf") {
                    Some("destructive command".to_owned())
                } else {
                    None
                })
            },
            block_reason_outcome,
        ))
        .with_harnesses(["fixture"]);

        let pass = command
            .run(&HookEvent {
                tool_command: Some("echo safe".to_owned()),
                ..hook_event()
            })
            .unwrap();
        let block = command
            .run(&HookEvent {
                tool_command: Some("rm -rf /".to_owned()),
                ..hook_event()
            })
            .unwrap();
        let missing = command.run(&hook_event()).unwrap_err();

        assert_eq!(command.name(), "guard-example");
        assert_eq!(command.stage(), HookStage::PreTool);
        assert!(command.is_enabled_for("fixture"));
        assert_eq!(pass, HookOutcome::pass());
        assert_eq!(block, HookOutcome::block("destructive command"));
        assert_eq!(missing.to_string(), "missing tool command");
    }

    #[test]
    fn runtime_services_are_created_lazily_per_run() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let service_calls = Arc::clone(&calls);
        let run_calls = Arc::clone(&calls);
        let run = with_runtime_services(
            move || {
                service_calls.lock().unwrap().push("services".to_owned());
                Ok("service".to_owned())
            },
            move |input: String, service: String| {
                run_calls.lock().unwrap().push(format!("run:{input}"));
                Ok(format!("{service}:{input}"))
            },
        );

        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(run("event".to_owned()).unwrap(), "service:event");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["services".to_owned(), "run:event".to_owned()]
        );
    }

    #[test]
    fn require_hook_event_field_reports_command_specific_errors() {
        let event = HookEvent {
            tool_call_id: Some("tool".to_owned()),
            ..hook_event()
        };

        assert_eq!(
            require_hook_event_field(&event, HookEventField::ToolCallId, "missing").unwrap(),
            HookEventFieldValue::String("tool")
        );
        assert_eq!(
            require_hook_event_field(
                &hook_event(),
                HookEventField::ToolCallId,
                "missing tool call"
            )
            .unwrap_err()
            .to_string(),
            "missing tool call"
        );
    }
}
