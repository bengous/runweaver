//! Where automation runs: the generic surface contract and the agent-hook
//! surface.
//!
//! A surface is an external event source — an agent harness hook, a Git
//! hook, a CI workflow. The generic contract in [`surface`] names triggers
//! ([`SurfaceTrigger`]) and shapes the data flow: a [`SurfaceCodec`] decodes
//! a native payload into a [`SurfaceEvent`] and encodes a [`SurfaceResponse`]
//! back. Triggers are what [`bindings`](crate::bindings) match on.
//!
//! [`agent_hooks`] is the fully-built surface for agent harnesses (Claude,
//! Codex, and custom ones): hook dispatch, codecs, command catalogs, and
//! generation of each harness's native hook configuration files.

pub mod agent_hooks;
pub mod surface;

pub use surface::{
    SurfaceCodec, SurfaceDecodeFn, SurfaceDefinition, SurfaceEncodeFn, SurfaceEvent,
    SurfaceResponse, SurfaceResponseStatus, SurfaceTrigger, define_surface,
};
