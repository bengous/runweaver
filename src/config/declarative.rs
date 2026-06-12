use std::{fmt, sync::Arc};

use super::validate::{format_binding_issues, validate_runweaver_definition};
use super::{
    ActionFn, ActionResult, ActionTask, ActionTaskOptions, CommandArgs, CommandTask,
    CommandTaskOptions, CompositeTaskOptions, ExecutionContext, ExitCodeRule,
    HostCommandDefinition, ManagedToolDefinition, ParallelTask, PolicyDefinition, PolicyVerdict,
    ResultMapping, RunweaverConfig, RunweaverDefinition, RunweaverOperationDefinition, SeriesTask,
    TaskDefinition, ToolConfig, ToolDefinition,
};
use crate::bindings::Binding;
use crate::diagnostics::{RunweaverDiagnostic, format_diagnostics, has_error_diagnostics};
use crate::surfaces::agent_hooks::{
    AgentHooksConfig, AgentHooksConfigDefinition, AgentHooksConfigError, AgentHooksConfigHook,
    Harness, HarnessCodec, HarnessTarget, HookApp, HookBinding, HookCommandSpec, HookEvent,
    HookOutcome, HookStage, define_agent_hooks_config,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolRef(&'static str);

impl ToolRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ToolRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyRef(&'static str);

impl PolicyRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for PolicyRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskRef(&'static str);

impl TaskRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for TaskRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationRef(&'static str);

impl OperationRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for OperationRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl From<OperationRef> for String {
    fn from(value: OperationRef) -> Self {
        value.as_str().to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HarnessRef(&'static str);

impl HarnessRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for HarnessRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HookCommandRef(&'static str);

impl HookCommandRef {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for HookCommandRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

pub const fn tool_ref(id: &'static str) -> ToolRef {
    ToolRef(id)
}

pub const fn policy_ref(id: &'static str) -> PolicyRef {
    PolicyRef(id)
}

pub const fn task_ref(id: &'static str) -> TaskRef {
    TaskRef(id)
}

pub const fn operation_ref(id: &'static str) -> OperationRef {
    OperationRef(id)
}

pub const fn harness_ref(id: &'static str) -> HarnessRef {
    HarnessRef(id)
}

pub const fn hook_command_ref(id: &'static str) -> HookCommandRef {
    HookCommandRef(id)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectBuildError {
    #[error("Duplicate Runweaver {category} ref: {id}")]
    DuplicateRef { category: &'static str, id: String },
    #[error("Invalid Runweaver project config:\n{diagnostics_text}")]
    InvalidConfig {
        diagnostics: Vec<RunweaverDiagnostic>,
        diagnostics_text: String,
    },
    #[error("Invalid Runweaver project bindings:\n{issues_text}")]
    InvalidBindings { issues_text: String },
}

/// The immutable result of [`ProjectBuilder::build`]: the project name, its
/// [`RunweaverDefinition`], and the derived task config.
#[derive(Debug, Clone)]
pub struct ProjectRunweaver {
    name: String,
    definition: RunweaverDefinition,
    config: RunweaverConfig,
}

impl ProjectRunweaver {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn task_config(&self) -> &RunweaverConfig {
        &self.config
    }

    pub fn runweaver_definition(&self) -> &RunweaverDefinition {
        &self.definition
    }

    pub fn into_task_config(self) -> RunweaverConfig {
        self.config
    }

    pub fn into_runweaver_definition(self) -> RunweaverDefinition {
        self.definition
    }
}

/// Declarative project composition root started by [`project`]. Typed refs
/// returned while registering tools, policies, tasks, and operations keep
/// cross-references checked; duplicate names and bindings to missing
/// operations are rejected at [`build`](Self::build).
#[derive(Debug, Clone)]
pub struct ProjectBuilder {
    name: String,
    definition: RunweaverDefinition,
    duplicate: Option<ProjectBuildError>,
}

/// Starts a named [`ProjectBuilder`].
pub fn project(name: impl Into<String>) -> ProjectBuilder {
    ProjectBuilder {
        name: name.into(),
        definition: RunweaverDefinition::new(),
        duplicate: None,
    }
}

#[derive(Clone)]
pub struct HookAppBuilder<'a> {
    name: String,
    binary_name: String,
    harnesses: Vec<&'a dyn HarnessCodec>,
    commands: Vec<HookCommandSpec>,
    duplicate: Option<ProjectBuildError>,
}

pub fn hook_app<'a>(name: impl Into<String>, binary_name: impl Into<String>) -> HookAppBuilder<'a> {
    HookAppBuilder {
        name: name.into(),
        binary_name: binary_name.into(),
        harnesses: Vec::new(),
        commands: Vec::new(),
        duplicate: None,
    }
}

#[derive(Clone)]
pub struct AgentHooksConfigBuilder<'a> {
    name: String,
    binary_name: String,
    source_path: String,
    harnesses: Vec<Harness<'a>>,
    targets: Vec<HarnessTarget>,
    hooks: Vec<AgentHooksConfigHook>,
}

pub fn agent_hooks_config<'a>(
    name: impl Into<String>,
    binary_name: impl Into<String>,
    source_path: impl Into<String>,
) -> AgentHooksConfigBuilder<'a> {
    AgentHooksConfigBuilder {
        name: name.into(),
        binary_name: binary_name.into(),
        source_path: source_path.into(),
        harnesses: Vec::new(),
        targets: Vec::new(),
        hooks: Vec::new(),
    }
}

impl<'a> HookAppBuilder<'a> {
    pub fn harness(&mut self, id: HarnessRef, codec: &'a dyn HarnessCodec) -> &mut Self {
        if self
            .harnesses
            .iter()
            .any(|candidate| candidate.harness() == id.as_str())
        {
            record_duplicate(&mut self.duplicate, "harness", id.as_str());
        }
        self.harnesses.push(codec);
        self
    }

    pub fn command(&mut self, command: impl Into<HookCommandSpec>) -> &mut Self {
        let command = command.into();
        if self
            .commands
            .iter()
            .any(|candidate| candidate.name() == command.name())
        {
            record_duplicate(&mut self.duplicate, "hook command", command.name());
        }
        self.commands.push(command);
        self
    }

    pub fn build(self) -> Result<HookApp<'a>, ProjectBuildError> {
        if let Some(error) = self.duplicate {
            return Err(error);
        }
        Ok(HookApp {
            name: self.name,
            binary_name: self.binary_name,
            harnesses: self.harnesses,
            commands: self.commands,
        })
    }
}

impl<'a> AgentHooksConfigBuilder<'a> {
    pub fn harness(&mut self, harness: Harness<'a>) -> &mut Self {
        self.harnesses.push(harness);
        self
    }

    pub fn target(&mut self, target: HarnessTarget) -> &mut Self {
        self.targets.push(target);
        self
    }

    pub fn hook(
        &mut self,
        command: impl Into<HookCommandSpec>,
        bindings: impl IntoIterator<Item = HookBinding>,
    ) -> &mut Self {
        self.hooks.push(AgentHooksConfigHook {
            command: command.into(),
            bindings: bindings.into_iter().collect(),
        });
        self
    }

    pub fn build(self) -> Result<AgentHooksConfig<'a>, AgentHooksConfigError> {
        define_agent_hooks_config(AgentHooksConfigDefinition::new(
            self.name,
            self.binary_name,
            self.source_path,
            self.harnesses,
            self.targets,
            self.hooks,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct HookCommandBuilder {
    spec: HookCommandSpec,
}

pub fn hook_command(
    id: HookCommandRef,
    stage: HookStage,
    run: impl Fn(&HookEvent) -> anyhow::Result<HookOutcome> + Send + Sync + 'static,
) -> HookCommandBuilder {
    HookCommandBuilder {
        spec: HookCommandSpec::new(id.as_str(), stage, run),
    }
}

impl HookCommandBuilder {
    pub fn harnesses(mut self, harnesses: impl IntoIterator<Item = HarnessRef>) -> Self {
        self.spec = self
            .spec
            .with_harnesses(harnesses.into_iter().map(|harness| harness.as_str()));
        self
    }
}

impl From<HookCommandBuilder> for HookCommandSpec {
    fn from(builder: HookCommandBuilder) -> Self {
        builder.spec
    }
}

impl ProjectBuilder {
    pub fn tools(mut self, configure: impl FnOnce(&mut ToolsBuilder<'_>)) -> Self {
        let mut builder = ToolsBuilder {
            definition: &mut self.definition,
            duplicate: &mut self.duplicate,
        };
        configure(&mut builder);
        self
    }

    pub fn policies(mut self, configure: impl FnOnce(&mut PoliciesBuilder<'_>)) -> Self {
        let mut builder = PoliciesBuilder {
            definition: &mut self.definition,
            duplicate: &mut self.duplicate,
        };
        configure(&mut builder);
        self
    }

    pub fn tasks(mut self, configure: impl FnOnce(&mut TasksBuilder<'_>)) -> Self {
        let mut builder = TasksBuilder {
            definition: &mut self.definition,
            duplicate: &mut self.duplicate,
        };
        configure(&mut builder);
        self
    }

    pub fn operations(mut self, configure: impl FnOnce(&mut OperationsBuilder<'_>)) -> Self {
        let mut builder = OperationsBuilder {
            definition: &mut self.definition,
            duplicate: &mut self.duplicate,
        };
        configure(&mut builder);
        self
    }

    pub fn bindings(mut self, configure: impl FnOnce(&mut BindingsBuilder<'_>)) -> Self {
        let mut builder = BindingsBuilder {
            definition: &mut self.definition,
        };
        configure(&mut builder);
        self
    }

    pub fn build(self) -> Result<ProjectRunweaver, ProjectBuildError> {
        if let Some(error) = self.duplicate {
            return Err(error);
        }
        let validation = validate_runweaver_definition(&self.definition);
        if has_error_diagnostics(&validation.config_diagnostics) {
            return Err(ProjectBuildError::InvalidConfig {
                diagnostics_text: format_diagnostics(&validation.config_diagnostics),
                diagnostics: validation.config_diagnostics,
            });
        }
        if !validation.binding_validation.ok {
            return Err(ProjectBuildError::InvalidBindings {
                issues_text: format_binding_issues(&validation.binding_validation.issues),
            });
        }
        let config = self.definition.task_config();
        Ok(ProjectRunweaver {
            name: self.name,
            definition: self.definition,
            config,
        })
    }
}

pub struct ToolsBuilder<'a> {
    definition: &'a mut RunweaverDefinition,
    duplicate: &'a mut Option<ProjectBuildError>,
}

impl ToolsBuilder<'_> {
    pub fn managed(&mut self, id: ToolRef, program: impl Into<String>) -> ManagedToolBuilder<'_> {
        self.insert(
            id,
            ToolDefinition::Tool(ManagedToolDefinition {
                program: program.into(),
                config: None,
            }),
        );
        ManagedToolBuilder {
            id,
            definition: self.definition,
        }
    }

    pub fn host(&mut self, id: ToolRef, program: impl Into<String>) -> &mut Self {
        self.insert(
            id,
            ToolDefinition::HostCommand(HostCommandDefinition {
                program: program.into(),
            }),
        );
        self
    }

    fn insert(&mut self, id: ToolRef, definition: ToolDefinition) {
        if self
            .definition
            .tools
            .insert(id.as_str().to_owned(), definition)
            .is_some()
        {
            record_duplicate(self.duplicate, "tool", id.as_str());
        }
    }
}

pub struct ManagedToolBuilder<'a> {
    id: ToolRef,
    definition: &'a mut RunweaverDefinition,
}

impl<'a> ManagedToolBuilder<'a> {
    pub fn config(self, flag: impl Into<String>, path: impl Into<String>) {
        if let Some(ToolDefinition::Tool(definition)) =
            self.definition.tools.get_mut(self.id.as_str())
        {
            definition.config = Some(ToolConfig {
                flag: flag.into(),
                path: path.into(),
            });
        }
    }
}

pub struct PoliciesBuilder<'a> {
    definition: &'a mut RunweaverDefinition,
    duplicate: &'a mut Option<ProjectBuildError>,
}

impl PoliciesBuilder<'_> {
    pub fn define(
        &mut self,
        id: PolicyRef,
        evaluate: impl Fn(&super::ExecutionContext) -> PolicyVerdict + Send + Sync + 'static,
    ) -> &mut Self {
        if self
            .definition
            .policies
            .insert(id.as_str().to_owned(), PolicyDefinition::new(evaluate))
            .is_some()
        {
            record_duplicate(self.duplicate, "policy", id.as_str());
        }
        self
    }
}

pub struct TasksBuilder<'a> {
    definition: &'a mut RunweaverDefinition,
    duplicate: &'a mut Option<ProjectBuildError>,
}

impl TasksBuilder<'_> {
    pub fn define(&mut self, id: TaskRef, task: impl Into<TaskDefinition>) -> &mut Self {
        if self
            .definition
            .tasks
            .insert(id.as_str().to_owned(), task.into())
            .is_some()
        {
            record_duplicate(self.duplicate, "task", id.as_str());
        }
        self
    }
}

pub struct OperationsBuilder<'a> {
    definition: &'a mut RunweaverDefinition,
    duplicate: &'a mut Option<ProjectBuildError>,
}

impl OperationsBuilder<'_> {
    pub fn define(
        &mut self,
        id: OperationRef,
        operation: impl Into<RunweaverOperationDefinition>,
    ) -> &mut Self {
        if self
            .definition
            .operations
            .insert(id.as_str().to_owned(), operation.into())
            .is_some()
        {
            record_duplicate(self.duplicate, "operation", id.as_str());
        }
        self
    }
}

pub struct BindingsBuilder<'a> {
    definition: &'a mut RunweaverDefinition,
}

impl BindingsBuilder<'_> {
    pub fn bind(&mut self, binding: Binding) -> &mut Self {
        self.definition.bindings.push(binding);
        self
    }
}

#[derive(Debug, Clone)]
pub struct CommandTaskBuilder {
    tool: ToolRef,
    options: CommandTaskOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOptions {
    args: CommandArgs,
    result: Option<ResultMapping>,
    policies: Vec<PolicyRef>,
}

impl CommandOptions {
    pub fn args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.args = CommandArgs::Static(
            args.into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect(),
        );
        self
    }

    pub fn command_args(mut self, args: CommandArgs) -> Self {
        self.args = args;
        self
    }

    pub fn dynamic_args(
        mut self,
        args: impl Fn(&ExecutionContext) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.args = CommandArgs::dynamic(args);
        self
    }

    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.policies = policies.into_iter().collect();
        self
    }

    pub fn result(mut self, result: impl Into<ResultMapping>) -> Self {
        self.result = Some(result.into());
        self
    }
}

impl Default for CommandOptions {
    fn default() -> Self {
        Self {
            args: CommandArgs::Static(Vec::new()),
            result: None,
            policies: Vec::new(),
        }
    }
}

pub fn command(tool: ToolRef) -> CommandTaskBuilder {
    CommandTaskBuilder {
        tool,
        options: CommandTaskOptions::new(CommandArgs::Static(Vec::new())),
    }
}

pub fn command_with(tool: ToolRef, options: CommandOptions) -> CommandTaskBuilder {
    CommandTaskBuilder {
        tool,
        options: CommandTaskOptions {
            args: options.args,
            result: options.result,
            policies: policy_strings(options.policies),
        },
    }
}

impl CommandTaskBuilder {
    pub fn args(mut self, args: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.options.args = CommandArgs::Static(
            args.into_iter()
                .map(|arg| arg.as_ref().to_owned())
                .collect(),
        );
        self
    }

    pub fn command_args(mut self, args: CommandArgs) -> Self {
        self.options.args = args;
        self
    }

    pub fn dynamic_args(
        mut self,
        args: impl Fn(&ExecutionContext) -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.options.args = CommandArgs::dynamic(args);
        self
    }

    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.options.policies = policy_strings(policies);
        self
    }

    pub fn result(mut self, result: impl Into<ResultMapping>) -> Self {
        self.options.result = Some(result.into());
        self
    }
}

impl From<CommandTaskBuilder> for TaskDefinition {
    fn from(builder: CommandTaskBuilder) -> Self {
        TaskDefinition::Command(CommandTask {
            tool: builder.tool.as_str().to_owned(),
            args: builder.options.args,
            result: builder.options.result,
            policies: builder.options.policies,
        })
    }
}

#[derive(Clone)]
pub struct ActionTaskBuilder {
    run: ActionFn,
    options: ActionTaskOptions,
}

impl fmt::Debug for ActionTaskBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActionTaskBuilder")
            .field("run", &"<fn>")
            .field("options", &self.options)
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionOptions {
    policies: Vec<PolicyRef>,
}

impl ActionOptions {
    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.policies = policies.into_iter().collect();
        self
    }
}

pub fn action(
    run: impl Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static,
) -> ActionTaskBuilder {
    ActionTaskBuilder {
        run: Arc::new(run),
        options: ActionTaskOptions::default(),
    }
}

pub fn action_with(
    run: impl Fn(&ExecutionContext) -> ActionResult + Send + Sync + 'static,
    options: ActionOptions,
) -> ActionTaskBuilder {
    ActionTaskBuilder {
        run: Arc::new(run),
        options: ActionTaskOptions {
            policies: policy_strings(options.policies),
        },
    }
}

impl ActionTaskBuilder {
    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.options.policies = policy_strings(policies);
        self
    }
}

impl From<ActionTaskBuilder> for TaskDefinition {
    fn from(builder: ActionTaskBuilder) -> Self {
        TaskDefinition::Action(ActionTask {
            run: builder.run,
            policies: builder.options.policies,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompositeKind {
    Series,
    Parallel,
}

#[derive(Debug, Clone)]
pub struct CompositeTaskBuilder {
    kind: CompositeKind,
    refs: Vec<String>,
    options: CompositeTaskOptions,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompositeOptions {
    fail_fast: bool,
    policies: Vec<PolicyRef>,
}

impl CompositeOptions {
    pub fn fail_fast(mut self) -> Self {
        self.fail_fast = true;
        self
    }

    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.policies = policies.into_iter().collect();
        self
    }
}

pub fn series(refs: impl IntoIterator<Item = TaskRef>) -> CompositeTaskBuilder {
    composite_task(CompositeKind::Series, refs)
}

pub fn parallel(refs: impl IntoIterator<Item = TaskRef>) -> CompositeTaskBuilder {
    composite_task(CompositeKind::Parallel, refs)
}

pub fn series_with(
    refs: impl IntoIterator<Item = TaskRef>,
    options: CompositeOptions,
) -> CompositeTaskBuilder {
    composite_task_with_options(CompositeKind::Series, refs, options)
}

pub fn parallel_with(
    refs: impl IntoIterator<Item = TaskRef>,
    options: CompositeOptions,
) -> CompositeTaskBuilder {
    composite_task_with_options(CompositeKind::Parallel, refs, options)
}

impl CompositeTaskBuilder {
    pub fn fail_fast(mut self) -> Self {
        self.options.fail_fast = true;
        self
    }

    pub fn policies(mut self, policies: impl IntoIterator<Item = PolicyRef>) -> Self {
        self.options.policies = policy_strings(policies);
        self
    }
}

impl From<CompositeTaskBuilder> for TaskDefinition {
    fn from(builder: CompositeTaskBuilder) -> Self {
        match builder.kind {
            CompositeKind::Series => TaskDefinition::Series(SeriesTask {
                refs: builder.refs,
                fail_fast: builder.options.fail_fast,
                policies: builder.options.policies,
            }),
            CompositeKind::Parallel => TaskDefinition::Parallel(ParallelTask {
                refs: builder.refs,
                fail_fast: builder.options.fail_fast,
                policies: builder.options.policies,
            }),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultMappingBuilder {
    mapping: ResultMapping,
}

pub fn result_mapping() -> ResultMappingBuilder {
    ResultMappingBuilder::default()
}

impl ResultMappingBuilder {
    pub fn success(mut self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.mapping.success = Some(codes.into_iter().collect());
        self
    }

    pub fn warning(mut self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.mapping.warning = Some(codes.into_iter().collect());
        self
    }

    pub fn error(mut self, rule: ExitCodeRule) -> Self {
        self.mapping.error = rule;
        self
    }

    pub fn tool_error(mut self, rule: ExitCodeRule) -> Self {
        self.mapping.tool_error = rule;
        self
    }

    pub fn error_codes(self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.error(ExitCodeRule::Codes(codes.into_iter().collect()))
    }

    pub fn tool_error_codes(self, codes: impl IntoIterator<Item = i32>) -> Self {
        self.tool_error(ExitCodeRule::Codes(codes.into_iter().collect()))
    }

    pub fn error_otherwise(self) -> Self {
        self.error(ExitCodeRule::Otherwise)
    }

    pub fn tool_error_otherwise(self) -> Self {
        self.tool_error(ExitCodeRule::Otherwise)
    }

    pub fn build(self) -> ResultMapping {
        self.mapping
    }
}

impl From<ResultMappingBuilder> for ResultMapping {
    fn from(builder: ResultMappingBuilder) -> Self {
        builder.build()
    }
}

pub fn success_codes(codes: impl IntoIterator<Item = i32>) -> ResultMapping {
    result_mapping().success(codes).build()
}

pub fn warning_codes(codes: impl IntoIterator<Item = i32>) -> ResultMapping {
    result_mapping().warning(codes).build()
}

pub fn error_codes(codes: impl IntoIterator<Item = i32>) -> ResultMapping {
    result_mapping().error_codes(codes).build()
}

pub fn tool_error_codes(codes: impl IntoIterator<Item = i32>) -> ResultMapping {
    result_mapping().tool_error_codes(codes).build()
}

pub fn error_otherwise() -> ExitCodeRule {
    ExitCodeRule::Otherwise
}

pub fn tool_error_otherwise() -> ExitCodeRule {
    ExitCodeRule::Otherwise
}

fn composite_task(
    kind: CompositeKind,
    refs: impl IntoIterator<Item = TaskRef>,
) -> CompositeTaskBuilder {
    composite_task_with_options(kind, refs, CompositeOptions::default())
}

fn composite_task_with_options(
    kind: CompositeKind,
    refs: impl IntoIterator<Item = TaskRef>,
    options: CompositeOptions,
) -> CompositeTaskBuilder {
    CompositeTaskBuilder {
        kind,
        refs: refs
            .into_iter()
            .map(|task_ref| task_ref.as_str().to_owned())
            .collect(),
        options: CompositeTaskOptions {
            fail_fast: options.fail_fast,
            policies: policy_strings(options.policies),
        },
    }
}

fn policy_strings(policies: impl IntoIterator<Item = PolicyRef>) -> Vec<String> {
    policies
        .into_iter()
        .map(|policy| policy.as_str().to_owned())
        .collect()
}

fn record_duplicate(duplicate: &mut Option<ProjectBuildError>, category: &'static str, id: &str) {
    if duplicate.is_none() {
        *duplicate = Some(ProjectBuildError::DuplicateRef {
            category,
            id: id.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::bind;
    use crate::config::{ExecutionContext, PolicyVerdict};
    use crate::core::OperationDefinition;
    use crate::surfaces::SurfaceTrigger;
    use crate::surfaces::agent_hooks::{
        HarnessDefinition, HarnessHookConfig, HarnessHookConfigRenderInput, HarnessTargetInput,
        HookBindingInput, define_harness,
    };

    const CARGO: ToolRef = tool_ref("cargo");
    const OXFMT: ToolRef = tool_ref("oxfmt");
    const FORMAT_CHECK: TaskRef = task_ref("formatCheck");
    const CARGO_CHECK: TaskRef = task_ref("cargoCheck");
    const CHECK: TaskRef = task_ref("check");
    const COUNT_FILES: OperationRef = operation_ref("countFiles");
    const HAS_RUST_FILES: PolicyRef = policy_ref("hasRustFiles");
    const CODEX: HarnessRef = harness_ref("codex");
    const GUARD: HookCommandRef = hook_command_ref("guard");

    struct TestCodec;

    impl HarnessCodec for TestCodec {
        fn harness(&self) -> &'static str {
            "codex"
        }

        fn decode(
            &self,
            _stdin: &str,
            stage: HookStage,
            _env: &crate::surfaces::agent_hooks::HookEnv,
        ) -> anyhow::Result<crate::surfaces::agent_hooks::HookRequest> {
            Ok(crate::surfaces::agent_hooks::HookRequest {
                event: crate::surfaces::agent_hooks::HookEvent {
                    harness: "codex".to_owned(),
                    stage,
                    session_id: "session".to_owned(),
                    tool_call_id: None,
                    transcript_path: None,
                    cwd: "/repo".to_owned(),
                    touched_path_candidates: Vec::new(),
                    patch_text: None,
                    tool_command: Some("pwd".to_owned()),
                    tool_name: None,
                    tool_response: None,
                    stop_hook_active: false,
                },
            })
        }

        fn encode(
            &self,
            outcome: HookOutcome,
            request: &crate::surfaces::agent_hooks::HookRequest,
        ) -> crate::surfaces::agent_hooks::HookEmission {
            crate::surfaces::agent_hooks::outcome_to_emission(request.event.stage, outcome)
        }

        fn encode_failure(
            &self,
            stage: HookStage,
            error: &anyhow::Error,
        ) -> crate::surfaces::agent_hooks::HookEmission {
            crate::surfaces::agent_hooks::HookEmission::block(stage, error.to_string())
        }
    }

    static TEST_CODEC: TestCodec = TestCodec;

    fn has_rust_files(ctx: &ExecutionContext) -> PolicyVerdict {
        if ctx.files.iter().any(|path| path.ends_with(".rs")) {
            PolicyVerdict::Allow
        } else {
            PolicyVerdict::Skip {
                reason: Some("No Rust files.".to_owned()),
            }
        }
    }

    fn ok_action(_: &ExecutionContext) -> crate::config::ActionResult {
        crate::config::ActionResult::success()
    }

    fn count_files_operation() -> OperationDefinition {
        OperationDefinition::new(|input, _services| {
            let count = input
                .get("files")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            Ok(serde_json::json!({ "count": count }))
        })
    }

    fn cli_trigger() -> SurfaceTrigger {
        SurfaceTrigger {
            surface: "cli".to_owned(),
            name: "count".to_owned(),
            phase: None,
        }
    }

    #[test]
    fn project_builder_creates_readable_task_config() {
        let project = project("fixture")
            .tools(|tools| {
                tools
                    .managed(OXFMT, "oxfmt")
                    .config("-c", ".runweaver/configs/oxfmtrc.jsonc");
                tools.host(CARGO, "cargo");
            })
            .policies(|policies| {
                policies.define(HAS_RUST_FILES, has_rust_files);
            })
            .tasks(|tasks| {
                tasks.define(
                    FORMAT_CHECK,
                    command(OXFMT).args(["--check"]).policies([HAS_RUST_FILES]),
                );
                tasks.define(
                    CARGO_CHECK,
                    command(CARGO).args(["check"]).policies([HAS_RUST_FILES]),
                );
                tasks.define(CHECK, parallel([FORMAT_CHECK, CARGO_CHECK]));
            })
            .build()
            .unwrap();

        let config = project.task_config();
        assert_eq!(project.name(), "fixture");
        assert!(config.tools.contains_key(OXFMT.as_str()));
        assert!(config.policies.contains_key(HAS_RUST_FILES.as_str()));
        let TaskDefinition::Parallel(check) = config.tasks.get(CHECK.as_str()).unwrap() else {
            panic!("check task should be parallel");
        };
        assert_eq!(check.refs, vec!["formatCheck", "cargoCheck"]);
    }

    #[test]
    fn project_builder_creates_runweaver_definition_with_operations_and_bindings() {
        let project = project("fixture")
            .tools(|tools| {
                tools.host(CARGO, "cargo");
            })
            .tasks(|tasks| {
                tasks.define(CARGO_CHECK, command(CARGO).args(["check"]));
            })
            .operations(|operations| {
                operations.define(COUNT_FILES, count_files_operation());
                operations.define(
                    operation_ref("cargoCheck"),
                    TaskDefinition::from(command(CARGO).args(["check"])),
                );
            })
            .bindings(|bindings| {
                bindings.bind(bind(cli_trigger()).to(COUNT_FILES).finish());
            })
            .build()
            .unwrap();

        let definition = project.runweaver_definition();

        assert!(definition.tasks.contains_key(CARGO_CHECK.as_str()));
        assert!(definition.operations.contains_key(COUNT_FILES.as_str()));
        assert!(definition.operations.contains_key("cargoCheck"));
        assert_eq!(definition.bindings[0].operation_name, COUNT_FILES.as_str());
    }

    #[test]
    fn project_builder_rejects_bindings_to_missing_operations_at_build_boundary() {
        let error = project("fixture")
            .bindings(|bindings| {
                bindings.bind(bind(cli_trigger()).to(COUNT_FILES).finish());
            })
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            ProjectBuildError::InvalidBindings {
                issues_text:
                    "Binding trigger cli/count at index 0 references missing operation \"countFiles\"."
                        .to_owned()
            }
        );
    }

    #[test]
    fn project_builder_reports_duplicate_refs_at_build_boundary() {
        let error = project("fixture")
            .tools(|tools| {
                tools.host(CARGO, "cargo");
                tools.host(CARGO, "cargo");
            })
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            ProjectBuildError::DuplicateRef {
                category: "tool",
                id: "cargo".to_owned()
            }
        );
    }

    #[test]
    fn project_builder_rejects_invalid_config_at_build_boundary() {
        let error = project("fixture")
            .tools(|tools| {
                tools.host(CARGO, "cargo");
            })
            .tasks(|tasks| {
                tasks.define(
                    FORMAT_CHECK,
                    command(OXFMT).args(["--check"]).policies([HAS_RUST_FILES]),
                );
                tasks.define(CHECK, parallel([FORMAT_CHECK, CARGO_CHECK]));
            })
            .build()
            .unwrap_err();

        let ProjectBuildError::InvalidConfig { diagnostics, .. } = error else {
            panic!("missing refs should be reported as invalid config");
        };
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();

        assert!(codes.contains(&"RUNWEAVER_TOOL_REF_MISSING"));
        assert!(codes.contains(&"RUNWEAVER_POLICY_REF_MISSING"));
        assert!(codes.contains(&"RUNWEAVER_TASK_REF_MISSING"));
    }

    #[test]
    fn result_mapping_builder_preserves_all_completion_rules() {
        let project = project("fixture")
            .tools(|tools| {
                tools.host(CARGO, "cargo");
            })
            .tasks(|tasks| {
                tasks.define(
                    CARGO_CHECK,
                    command(CARGO).result(
                        result_mapping()
                            .success([0])
                            .warning([2])
                            .error_codes([3])
                            .tool_error_otherwise(),
                    ),
                );
            })
            .build()
            .unwrap();

        let TaskDefinition::Command(task) = project
            .task_config()
            .tasks
            .get(CARGO_CHECK.as_str())
            .unwrap()
        else {
            panic!("cargoCheck task should be a command");
        };
        assert_eq!(
            task.result,
            Some(ResultMapping {
                success: Some(vec![0]),
                warning: Some(vec![2]),
                error: ExitCodeRule::Codes(vec![3]),
                tool_error: ExitCodeRule::Otherwise,
            })
        );
    }

    #[test]
    fn task_option_constructors_preserve_typed_refs_and_options() {
        let project = project("fixture")
            .tools(|tools| {
                tools.host(CARGO, "cargo");
                tools
                    .managed(OXFMT, "oxfmt")
                    .config("-c", ".runweaver/configs/oxfmtrc.jsonc");
            })
            .policies(|policies| {
                policies.define(HAS_RUST_FILES, has_rust_files);
            })
            .tasks(|tasks| {
                tasks.define(
                    FORMAT_CHECK,
                    command_with(
                        OXFMT,
                        CommandOptions::default()
                            .args(["--check"])
                            .policies([HAS_RUST_FILES])
                            .result(warning_codes([2])),
                    ),
                );
                tasks.define(
                    CARGO_CHECK,
                    action_with(
                        ok_action,
                        ActionOptions::default().policies([HAS_RUST_FILES]),
                    ),
                );
                tasks.define(
                    CHECK,
                    series_with(
                        [FORMAT_CHECK, CARGO_CHECK],
                        CompositeOptions::default()
                            .fail_fast()
                            .policies([HAS_RUST_FILES]),
                    ),
                );
            })
            .build()
            .unwrap();
        let config = project.task_config();

        let TaskDefinition::Command(format_check) =
            config.tasks.get(FORMAT_CHECK.as_str()).unwrap()
        else {
            panic!("formatCheck task should be a command");
        };
        let TaskDefinition::Action(cargo_check) = config.tasks.get(CARGO_CHECK.as_str()).unwrap()
        else {
            panic!("cargoCheck task should be an action");
        };
        let TaskDefinition::Series(check) = config.tasks.get(CHECK.as_str()).unwrap() else {
            panic!("check task should be a series");
        };

        assert_eq!(
            format_check.args,
            CommandArgs::Static(vec!["--check".to_owned()])
        );
        assert_eq!(format_check.policies, vec!["hasRustFiles".to_owned()]);
        assert_eq!(format_check.result, Some(warning_codes([2])));
        assert_eq!(cargo_check.policies, vec!["hasRustFiles".to_owned()]);
        assert!(check.fail_fast);
        assert_eq!(check.policies, vec!["hasRustFiles".to_owned()]);
        assert_eq!(check.refs, vec!["formatCheck", "cargoCheck"]);
    }

    #[test]
    fn hook_app_builder_creates_typed_hook_app() {
        let mut builder = hook_app("fixture", "runweaver hook");
        builder.harness(CODEX, &TEST_CODEC);
        builder.command(hook_command(GUARD, HookStage::PreTool, |_| {
            Ok(HookOutcome::pass())
        }));
        let app = builder.build().unwrap();

        assert_eq!(app.name, "fixture");
        assert!(app.harness("codex").is_ok());
        assert!(app.command("guard", "codex").is_ok());
    }

    #[test]
    fn hook_app_builder_reports_duplicate_commands() {
        let mut builder = hook_app("fixture", "runweaver hook");
        builder.harness(CODEX, &TEST_CODEC);
        builder.command(hook_command(GUARD, HookStage::PreTool, |_| {
            Ok(HookOutcome::pass())
        }));
        builder.command(hook_command(GUARD, HookStage::PreTool, |_| {
            Ok(HookOutcome::pass())
        }));

        let Err(error) = builder.build() else {
            panic!("duplicate hook command should fail");
        };
        assert_eq!(
            error,
            ProjectBuildError::DuplicateRef {
                category: "hook command",
                id: "guard".to_owned()
            }
        );
    }

    fn test_harness() -> Harness<'static> {
        define_harness(HarnessDefinition {
            id: "codex".to_owned(),
            codec: &TEST_CODEC,
            hook_config: HarnessHookConfig::new(
                ".codex/config.toml",
                |_input: HarnessHookConfigRenderInput<'_>| Ok("hooks\n".to_owned()),
            ),
            agents_surface: crate::surfaces::agent_hooks::AgentsSurfaceDefaults::new(
                crate::surfaces::agent_hooks::RunweaverHookCommandCwd::None,
            ),
        })
    }

    #[test]
    fn agent_hooks_config_builder_creates_runtime_app_and_hook_config() {
        let harness = test_harness();
        let mut builder = agent_hooks_config("fixture hooks", "fixture hook", "hooks.rs");
        builder.harness(harness.clone());
        builder.target(harness.target(HarnessTargetInput::new("fixture hook codex")));
        builder.hook(
            hook_command(GUARD, HookStage::PreTool, |_| Ok(HookOutcome::pass())),
            [harness.bind(HookBindingInput::new(10, "Guard").with_matcher("Bash"))],
        );

        let config = builder.build().unwrap();

        assert_eq!(config.name, "fixture hooks");
        assert_eq!(config.source_path, "hooks.rs");
        assert!(config.app.command("guard", "codex").is_ok());
        assert_eq!(config.harness_hook_config.targets[0].harness, "codex");
    }
}
