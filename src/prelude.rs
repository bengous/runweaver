//! One-stop imports for project authors.
//!
//! Re-exports the authoring core: assemble a [`RunweaverDefinition`] with
//! [`project`] or [`define_runweaver_with`], load or write manifests against
//! a [`BuiltinRegistry`], and compose a [`CompiledRunweaverProject`].
//! `use runweaver::prelude::*;` covers typical project configuration code;
//! reach into specific modules only for deeper integration work.

pub use crate::cli::{
    CompiledRunweaverProject, CompiledRunweaverProjectBuilder, compiled_runweaver_project,
};
pub use crate::config::{
    BuiltinRegistry, ExecutionContext, LoadedRunweaverManifest, ManifestLoadError, RunweaverConfig,
    RunweaverDefinition, RunweaverDefinitionManifest, RunweaverProjectBinary,
    default_builtin_registry, define_runweaver_with, load_runweaver_manifest, project,
};
