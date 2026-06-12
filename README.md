# Runweaver

Runweaver declares a project's quality tooling once and consumes it everywhere
the project needs enforcement: agent hooks for Claude, Codex, and custom harnesses, Git
hooks, CI workflows, and command-line entrypoints.

The crate is a Rust library and generic binary foundation for project quality
automation. It is not a linter distribution and does not bundle tools. Projects
choose their own tools; Runweaver scopes, runs, composes, and projects their
outcomes into the native surface that invoked them.

## User Concepts

Runweaver keeps the user-facing model to four concepts.

- **Paths** define write zones. Guards and fix-to-check degradation derive from
  the path model instead of from surface-specific rules.
- **Tool** defines check and fix modes, target file types, command execution,
  and diagnostics parsing for one project tool.
- **Pipeline** composes tools in a chosen mode for a quality gate such as
  check, validate, or autofix.
- **Surface** defines where a pipeline is consumed, including input scoping and
  projection into an agent hook, Git hook, CI job, or CLI command.

Everything else is library machinery: manifests, operations, profiles, codecs,
runtime services, bindings, and generated files.

## Architecture

The intended hot path is data-driven:

```text
config (TypeScript data literal)
  -- runweaver sync --> manifest (JSON)
  -- read at startup --> generic Rust binary
```

The TypeScript layer is a closure-free data literal evaluated at sync time.
The manifest is pure JSON. The Rust binary reads that manifest at startup and
executes the selected surface without evaluating TypeScript during hooks.

The orchestrator target is under 5 ms over invoking the underlying tool
directly. Tool startup time belongs to the tool, not to Runweaver.

## Outcome Contract

Surfaces consume one common outcome shape:

- `pass`
- `fail(diagnostics)`
- `fixed(changedFiles)`

Each surface projects that contract into its own protocol: agent-loop feedback,
Git hook abort or restage behavior, CI annotations, or CLI exit status.

## Library And Binary Roles

The library exposes the definition, manifest, runtime, service, surface, and
embedded-binary primitives used by project-specific Runweaver integrations.

The generic binary path is designed for projects to inject identity and
project-owned behavior through `RunweaverProjectBinary` composition. The crate
owns the reusable runtime and surface machinery; the project binary owns the
project name, manifest location, builtin registry, and any repository-specific
commands.

## More Context

- `examples/` contains design examples and primitive evaluations.
- The crate-level rustdoc in `src/lib.rs` is the API map for library users.
