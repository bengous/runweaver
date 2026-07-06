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

Everything else is library machinery: manifests, operations, profiles,
runtime services, bindings, and generated files.

## Architecture

The hot path is data-driven:

```text
runweaver.config.ts (closure-free TypeScript data literal)
  -- evaluated by bun, piped as JSON --> runweaver sync manifest
  -- .runweaver/manifest.json, read at startup --> generic Rust binary
```

The TypeScript layer is a plain data literal typed with `satisfies` against
generated declarations. Runweaver never evaluates TypeScript: `runweaver sync
manifest` reads JSON from stdin, validates it against the manifest schema, and
writes `.runweaver/manifest.json`. The Rust binary reads that manifest at
startup and executes the selected surface; no TypeScript runs during hooks.

## Authoring a Manifest

`runweaver manifest types` writes `.runweaver/manifest.d.ts`, TypeScript
declarations generated from the Rust manifest JSON schema. The Rust schema is
the single source of truth; rerun the command after upgrading `runweaver` to
pick up schema changes (the `schema-sha256` stamp in the banner identifies the
generating schema). There is no published npm package.

End to end:

```console
$ runweaver manifest types
Wrote .runweaver/manifest.d.ts
```

`runweaver.config.ts` — a data literal that prints itself as JSON:

```typescript
import type { RunweaverDefinitionManifest } from "./.runweaver/manifest.d.ts";

const manifest = {
  version: 2,
  paths: { writable: ["src/"] },
  tools: {
    fmtCheck: { script: "cargo fmt --check" },
  },
  pipelines: {
    check: { check: ["fmtCheck"] },
  },
  operations: {},
  surfaces: {
    agents: {
      harnesses: ["claude", "codex"],
      preTool: [{ guard: "destructive-commands" }],
      stop: { run: "check" },
    },
    git: { preCommit: { run: "check" } },
    cli: true,
  },
  bindings: [],
} satisfies RunweaverDefinitionManifest;

console.log(JSON.stringify(manifest));
```

Sync, then run:

```console
$ bun run runweaver.config.ts | runweaver sync manifest
Wrote .runweaver/manifest.json
$ runweaver run check
check: success
```

`bun run` evaluates but does not typecheck; the `satisfies` clause is enforced
by the editor or `bunx tsc --noEmit --strict runweaver.config.ts`. Drift
between config and committed manifest is detected the same way it is written:

```console
$ bun run runweaver.config.ts | runweaver check manifest
```

The default builtin registry ships two harnesses (Claude, Codex) and the
`destructive-commands` guard. Custom operations require compiling a project
binary (see below).

## Performance

The orchestrator target is under 5 ms of overhead over invoking the underlying
tool directly, and the measurements substantiate it. On a warm Linux
workstation (median of 100 iterations after 10 warmups, release build,
`scripts/bench-hot-path.py`), the generic binary added 1.4–2 ms of
orchestrator overhead over the matching bare-spawn baseline (`/usr/bin/true`
for hook dispatch, `sh -c true` for task and Git pre-commit runs), landing at
2–3 ms end to end per invocation. That overhead includes a full manifest
re-parse and re-validation every time. Manifest load scales sub-linearly: a
10x manifest (100 tools, 100 pipelines) added 0.5–0.75 ms across runs. p95
stayed under 5 ms end to end in every measured scenario. Exact numbers vary
run to run.

Reproduce with:

```console
$ cargo build --release
$ python3 scripts/bench-hot-path.py target/release/runweaver
```

Tool startup time belongs to the tool, not to Runweaver.

## Outcome Contract

Surfaces consume one common outcome shape:

- `pass`
- `fail(diagnostics)`
- `fixed(changedFiles)`

Each surface projects that contract into its own protocol: agent-loop feedback,
Git hook abort or restage behavior, CI annotations, or CLI exit status.

## Library And Binary Roles

The library exposes the definition, manifest, runtime, service, and
embedded-binary primitives used by project-specific Runweaver integrations.

The generic binary path is designed for projects to inject identity and
project-owned behavior through `RunweaverProjectBinary` composition. The crate
owns the reusable runtime and surface machinery; the project binary owns the
project name, manifest location, builtin registry, and any repository-specific
commands.

## More Context

- `examples/` contains design documents and primitive evaluations, not a
  runnable workflow. They import from an aspirational `@bengous/runweaver`
  package that does not exist and is not published; the shipped authoring path
  is the generated `.runweaver/manifest.d.ts` flow described above.
- The crate-level rustdoc in `src/lib.rs` is the API map for library users.
