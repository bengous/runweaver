//! Execution engine for tasks and operations.
//!
//! [`run_task`] executes a named task from a
//! [`RunweaverConfig`](crate::config::RunweaverConfig) inside an
//! [`ExecutionContext`](crate::config::ExecutionContext) (built with
//! [`create_execution_context`]) and returns the full
//! [`TaskRun`](crate::config::TaskRun) tree: per-task status, completion,
//! output, and child runs for series/parallel composites. Policies are
//! evaluated before any work; denied or skipped tasks record their reason
//! instead of a completion.
//!
//! Composite semantics live in the aggregation helpers:
//! [`aggregate_task_completion`] folds child completions (tool error >
//! error > warning > success), [`is_blocking_run`] classifies a run as one
//! that should fail a pipeline or block a hook, and series steps can chain
//! context mutations through
//! [`NextExecutionContext`](crate::config::NextExecutionContext).
//!
//! Operations and bindings execute through [`run_runweaver_operation`],
//! [`run_bound_runweaver_operation`], and [`run_resolved_runweaver_binding`],
//! which resolve names against a
//! [`RunweaverDefinition`](crate::config::RunweaverDefinition) and apply bound
//! profiles; [`run_runweaver_operation_as_json`] bridges results into plain
//! JSON for process boundaries.
//!
//! For model-visible output, [`compact_run_for_agents`] reduces a run tree to
//! the failures that matter and [`format_notable_runs`] renders them as text.

pub(crate) mod context;
pub(crate) mod format;
pub(crate) mod operation_runner;
pub(crate) mod result;
pub(crate) mod task_runner;

pub use context::{CreateExecutionContextOptions, create_execution_context, normalize_files};
pub use format::{
    CompactTaskOutput, CompactTaskRun, compact_run_for_agents, format_notable_runs,
    task_run_result_label,
};
pub use operation_runner::{
    RunweaverOperationRunError, RunweaverOperationRunResult, run_bound_runweaver_operation,
    run_resolved_runweaver_binding, run_runweaver_operation, run_runweaver_operation_as_json,
};
pub use result::{
    aggregate_task_completion, aggregate_task_output, is_blocking_run, map_task_completion,
};
pub use task_runner::run_task;
