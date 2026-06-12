// @ts-nocheck — design artifact: the import below does not exist yet.
/**
 * SECOND PROJECT (Python) — Phase 3 design artifact, NOT executable.
 *
 * Full dream config for a realistic second project with a disjoint stack:
 * a Python service managed with uv (src/ + tests/ layout, alembic
 * migrations), quality tooling ruff + mypy + pytest. It intentionally uses
 * a different toolchain (no bun, no cargo, no oxc) to falsify the
 * 4-concept model and the primitive set on a foreign stack. Every primitive
 * used here is a second-consumer
 * vote for PRIMITIVES.md; every primitive this file CANNOT use honestly is
 * a cut vote.
 *
 * Same constraints as target-config.example.ts: data literal, closure-free,
 * four concepts, presets live in the Rust binary.
 */

import { defineRunweaver } from "@bengous/runweaver";

export default defineRunweaver({
  // ── Paths ─────────────────────────────────────────────────────────────────
  // `readOnly`: applied alembic migrations are immutable history — agents
  // must add new revisions, never edit old ones. Second real consumer of the
  // readOnly zone.
  // No `checkOnly` zone here: this repo has no codex-style half-writable
  // area. Honest non-use — counts against checkOnly in the verdict table.
  paths: {
    writable: ["src/", "tests/", "scripts/", "pyproject.toml"],
    generated: ["uv.lock"],
    readOnly: ["migrations/versions/"],
  },

  // ── Tools ─────────────────────────────────────────────────────────────────
  tools: {
    // ruff is two tools in one binary; the presets keep check/fix knowledge:
    // ruff.lint  check: `ruff check`        fix: `ruff check --fix`
    // ruff.fmt   check: `ruff format --check`  fix: `ruff format`
    // Both read [tool.ruff] from pyproject.toml natively — no `config` field
    // needed anywhere in this file.
    "ruff.lint": { preset: "ruff" },
    "ruff.fmt": { preset: "ruff-format" },

    // Whole-program, ignores file scoping — same preset behavior as tsc.
    mypy: { preset: "mypy", args: ["--strict"] },

    // `affected` works only as far as the repo's naming convention does:
    // src/pkg/orders.py -> tests/test_orders.py / tests/**/test_orders.py.
    // Import-graph selection (pytest-testmon) is NOT expressible as path
    // patterns and must not become one — if a repo wants it, testmon is the
    // tool and `affected` is simply omitted. Convention-bound but honest:
    // second consumer of `affected` and of the {stem}/{dir} placeholders.
    pytest: {
      preset: "pytest",
      targets: ["tests/"],
      affected: ["tests/test_{stem}.py", "tests/{dir}/test_{stem}.py"],
    },

    gitleaks: { preset: "gitleaks" },

    // Declarative entries (the middle ladder rung), no preset involved.
    // deptry: dependency hygiene (unused/missing/transitive deps).
    deptry: {
      targets: { extensions: ["py"] },
      check: ["deptry", "src"],
      diagnostics: {
        parser: "regex",
        pattern: "^(?<file>.+?):(?<line>\\d+):(?<col>\\d+): (?<code>DEP\\d+) (?<message>.+)$",
      },
    },
    // committed: conventional-commit message check, single Rust binary —
    // no node dependency for the git hook path. NOTE the missing input:
    // a commit-msg tool needs the message FILE, not `{files}`. Rather than
    // mint a {commitMsgFile} placeholder for one consumer, the commitMsg
    // slot appends the message file path as the trailing argument by git
    // convention ($1). Recorded in PRIMITIVES.md.
    committed: {
      check: ["committed", "--commit-msg-file"],
      diagnostics: { parser: "unix" },
    },

    // Script rung: lockfile drift. uv exits non-zero if uv.lock is stale
    // against pyproject.toml.
    lockCheck: { script: "uv lock --check" },
  },

  // ── Pipelines ─────────────────────────────────────────────────────────────
  // Two stages: fast correctness first, slower hygiene second.
  pipelines: {
    check: { check: ["ruff.fmt", "ruff.lint", "mypy", "pytest"] },
    hygiene: { check: ["deptry", "lockCheck"] },
    validate: { stages: ["check", "hygiene"] },
    autofix: { fix: ["ruff.lint", "ruff.fmt"], then: { check: ["ruff.lint"] } },
  },

  // ── Surfaces ──────────────────────────────────────────────────────────────
  surfaces: {
    // Subset of harnesses — proves the list is config, not a constant.
    // No pi on this project. No `overrides` needed (counts against it).
    agents: {
      harnesses: ["claude", "codex"],
      preTool: [{ guard: "destructive-commands" }, { guard: "secrets", tool: "gitleaks" }],
      postEdit: { run: "autofix", timeout: 60 },
      stop: { run: "validate", timeout: 300 },
    },

    git: {
      preCommit: { run: "autofix", files: "staged", also: ["gitleaks"] },
      commitMsg: { tool: "committed" },
      prePush: { run: "validate" },
    },

    ci: { github: { pullRequest: "validate" } },

    cli: true,
  },
});

/**
 * ── Findings (input to PRIMITIVES.md and the Phase 4 go/no-go) ────────────
 *
 * 1. The four concepts held. Nothing in a uv/ruff/mypy/pytest repo asked for
 *    a fifth concept, a closure, or a new guard KIND (the set stays closed
 *    at: path-derived, tool-backed, builtin destructive-commands — hook-1
 *    tripwire NOT triggered).
 * 2. Adoption friction, recorded not hidden: this Python service needs node/bun at
 *    SYNC TIME ONLY to evaluate this .ts file (decided 2026-06-10: types
 *    generated from the Rust schema). Hook time is pure Rust. If this ever
 *    blocks a real adoption, the fallback is writing the manifest JSON
 *    directly — the TS layer is ergonomics, not capability.
 * 3. Honest non-uses (cut votes): `config` field (pyproject autodiscovery),
 *    `checkOnly` zone, per-harness `overrides`, script `variants`,
 *    pipeline wildcards ("cargo.*"-style), `denyWarnings`.
 * 4. Second-consumer votes (keep): readOnly zone, generated zone, preset,
 *    args, targets, affected + {stem}/{dir}, declarative entries +
 *    diagnostics parsers, script rung, stages, fix+then, check, guards
 *    (both kinds used), postEdit/stop/timeout, git preCommit
 *    files:"staged"/also/commitMsg/prePush, ci, cli.
 * 5. `{files}` in script entries still has ONE consumer: lockCheck and
 *    deptry run repo-wide by nature. Stays
 *    flagged in the verdict table.
 * 6. commit-msg input: solved by trailing-argument convention (git's $1)
 *    instead of a {commitMsgFile} placeholder — one less primitive.
 */
