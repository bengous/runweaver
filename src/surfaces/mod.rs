//! Where automation runs: surface triggers and the agent-hook surface.
//!
//! A surface is an external event source — an agent harness hook, a Git
//! hook, a CI workflow. A [`SurfaceTrigger`] names one event source; triggers
//! are what [`bindings`](crate::bindings) match on.
//!
//! [`agent_hooks`] is the fully-built surface for agent harnesses (Claude,
//! Codex, and custom ones): hook dispatch, codecs, command catalogs, and
//! generation of each harness's native hook configuration files.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod agent_hooks;

/// Identifies one event source: a surface name (e.g. `"agent-hook"`), a
/// trigger name (e.g. `"post-edit"`), and an optional phase. Bindings match
/// triggers exactly, including the phase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceTrigger {
    pub surface: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}
