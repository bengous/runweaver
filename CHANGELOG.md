# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The manifest tool layer now supports `cargo fmt` check mode (`cargo fmt --
  --check`), package-scoped with `-p <package>` when touched files map to
  `crates/<package>/` and `--all` otherwise, aligning the manifest-driven
  format gate with CI.

### Removed

- The `examples/` design-document directory: aspirational
  `@bengous/runweaver` API sketches superseded by the shipped
  `.runweaver/manifest.d.ts` codegen path.

## [0.2.0] - 2026-07-06

### Changed

- The public API is trimmed to the consumed surface: crate-root and prelude
  re-exports now carry what real consumers and the crate documentation
  reference, plus the companion types their signatures require. Internal
  machinery drops to crate visibility.
- The compiled-CLI entry points collapse to the project-based forms
  (`run_compiled_runweaver_project_cli`, its `_with_compile` variant, and
  `run_generic_runweaver_cli`).
- The declared minimum supported Rust version is 1.88, the oldest toolchain
  that builds the current dependency graph; edition 2024 alone had allowed an
  undeliverable 1.85.
- The README documents the shipped authoring path — a TypeScript data literal
  typed with `satisfies` against the generated `.runweaver/manifest.d.ts`,
  evaluated by bun and piped into `runweaver sync manifest` — and quotes
  measured hot-path numbers instead of a bare target.

### Removed

- The unused generic surface abstraction (`SurfaceCodec`, `SurfaceEvent`,
  `SurfaceResponse`, `SurfaceDefinition`, `define_surface`, and related types)
  and the `resolve_binding` wrapper. `SurfaceTrigger` remains, relocated to
  `surfaces::mod`. `resolve_binding_trigger` is the binding-resolution entry
  point.

### Added

- `scripts/bench-hot-path.py`, a self-contained benchmark of orchestrator
  overhead for the generic binary, referenced from the README.

## [0.1.0] - 2026-06-12

Initial release: manifest-driven quality automation projected into agent
hooks (Claude, Codex), Git hooks, CI workflows, and CLI commands, with the
library primitives for project-specific binaries.
