//! Support for project-compiled Runweaver binaries.
//!
//! A project can compile its definition into its own Rust binary instead of
//! loading config at runtime. This module provides the three pieces that
//! make such binaries self-sufficient:
//!
//! - **Embedded CLI** — [`run_embedded_runweaver_cli`] dispatches the reduced
//!   command set (`check`, `check binary`, `check hooks`, `sync hooks`,
//!   `hook`, `run`) against an [`EmbeddedRunweaverRuntime`] holding the
//!   compiled config, hook config, and binary manifest.
//! - **Compilation** — [`compile_cargo_runweaver_binary`] runs
//!   `cargo build --release` for the project package, copies the binary to
//!   its install path, and produces the manifest.
//! - **Fingerprinting** — a [`RunweaverBinaryManifest`] records the source
//!   files ([`RunweaverBinaryManifestInput`]) that went into a build and a
//!   deterministic digest over them ([`fingerprint_manifest_inputs`]).
//!   `check binary` recomputes the fingerprint from the current sources to
//!   detect a stale compiled binary. [`RUNWEAVER_BINARY_MANIFEST_VERSION`]
//!   guards the manifest format.

pub mod cli;
pub mod compile;
pub mod fingerprint;

pub use cli::{
    EmbeddedRunweaverCliIo, EmbeddedRunweaverJsonMode, EmbeddedRunweaverParsedOptions,
    EmbeddedRunweaverRuntime, EmbeddedRunweaverStdin, embedded_runweaver_help_text,
    parse_embedded_runweaver_options, run_embedded_runweaver_cli,
};
pub use compile::{
    CompileCargoRunweaverBinaryError, CompileCargoRunweaverBinaryOptions,
    CompileCargoRunweaverBinaryResult, compile_cargo_runweaver_binary,
};
pub use fingerprint::{
    RUNWEAVER_BINARY_MANIFEST_VERSION, RunweaverBinaryManifest, RunweaverBinaryManifestInput,
    RunweaverFingerprintError, create_runweaver_binary_manifest, fingerprint_manifest_inputs,
    read_runweaver_binary_manifest_inputs,
};
