//! The operation primitive: a JSON-in/JSON-out unit of work.
//!
//! An [`OperationDefinition`] wraps a closure that receives a
//! [`serde_json::Value`] input plus injected
//! [`RunweaverServices`](crate::services::RunweaverServices) and returns a JSON
//! output or an [`OperationError`]. Operations are what
//! [`bindings`](crate::bindings) route surface events to, and what
//! [`profiles`](crate::profiles) wrap with before/after/on-error middleware.
//!
//! Operations registered on a
//! [`RunweaverDefinition`](crate::config::RunweaverDefinition) may also be
//! backed by tasks; see
//! [`RunweaverOperationDefinition`](crate::config::RunweaverOperationDefinition).

pub(crate) mod operation;

pub use operation::{OperationDefinition, OperationError, OperationExecuteFn, define_operation};
