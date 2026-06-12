# Primitives — keep/cut verdicts (Phase 3)

Criterion: a primitive is **keep** only if it appears
in ≥2 real configs AND deletes lines versus writing without it. Otherwise
**cut**. Two real configs exist:

- **A** — TypeScript service: `target-config.example.ts` + `future-hooks.example.ts`
  (10 hooks, batch 1 ratified 2026-06-10)
- **B** — Python service: `python-project.example.ts` (uv/ruff/mypy/pytest)

Counting rule: commented illustrations (the biome block in A) are not real
usage. A `⚠` marks verdicts where the strict criterion produces a consequence
the user must arbitrate in Phase 4.

## Paths

| Primitive          | A   | B   | Deletes lines                       | Verdict |
| ------------------ | --- | --- | ----------------------------------- | ------- |
| `writable` zones   | ✓   | ✓   | 3 fileTargetPolicy gates, constants | keep    |
| `checkOnly` zones  | ✓   | —   | formatCheckOnly task in A           | keep    |
| `generated` zones  | ✓   | ✓   | guard-edit-paths binding code       | keep    |
| `readOnly` zones   | ✓   | ✓   | guard-reference-repos hook (15 l.)  | keep    |

`checkOnly` — ruled in Phase 4 (2026-06-10): **keep**. One production
consumer is not speculative machinery; the cost is a fourth zone kind inside
Paths, not a new concept. Cutting would force `.codex/` into a behavior A
does not have today.

## Tool

| Primitive                                | A   | B   | Deletes lines                      | Verdict |
| ---------------------------------------- | --- | --- | ---------------------------------- | ------- |
| `preset`                                 | ✓   | ✓   | per-tool args/parsing/task trios   | keep    |
| `args`                                   | ✓   | ✓   | —needed escape for preset defaults | keep    |
| `targets`                                | ✓   | ✓   | hand-rolled fileTargets sets       | keep    |
| `affected` + `{stem}`/`{dir}` patterns   | ✓   | ✓   | file→test mapping helpers          | keep ⚠  |
| `config`                                 | ✓   | —   | flag plumbing in A (8 uses)        | cut ⚠   |
| `denyWarnings` (cargo.clippy sugar)      | ✓   | —   | 0 (vs `args: ["--","-D","warnings"]`) | cut  |
| declarative entry (`check`/`fix`)        | —*  | ✓   | the alternative is preset sprawl   | keep    |
| `diagnostics` parser specs (unix/regex/…) | —*  | ✓   | per-tool output parsers            | keep    |
| `script` entries                         | ✓×6 | ✓   | command() boilerplate per script   | keep    |
| `variants` on scripts                    | ✓   | —   | 2 lines in A                       | cut     |
| `daemon` (warm-tool invocation)          | —   | —   | nothing yet                        | cut     |
| `{files}` in script/declarative entries  | ✓×1 | —   | session-scoped doc-drift needs it  | cut     |

\* A's biome block is a commented illustration, not real usage.

⚠ `affected`: B's usage is convention-bound (`tests/test_{stem}.py`); it
under-selects on cross-module impact. The stop-time full run covers the gap.
Keep, knowing its honest precision.

⚠ `config`: strict criterion cuts it — B's whole stack autodiscovers
pyproject.toml. Replacement is lossless: `args: ["-c", "path"]` per tool.
Recommended: **cut, fold into `args`** (one less field, zero capability lost).

declarative entries + `diagnostics` — ruled in Phase 4 (2026-06-10):
**keep by charter**, the one deliberate exception to the ≥2-consumers
criterion. This is the tool-agnosticism contract itself:
"defineTool is the extension point"; cutting it would make the library
preset-only — a charter violation, not a simplification.

`{files}` in scripts — ruled in Phase 4 (2026-06-10): **cut for now**.
docDrift degrades to computing its own git diff inside the script — an
approximation of session touched-paths (wrong under parallel worktrees).
First candidate for re-introduction the moment a second consumer appears.

## Pipeline

| Primitive                      | A   | B   | Deletes lines                     | Verdict |
| ------------------------------ | --- | --- | --------------------------------- | ------- |
| `check` (parallel, check mode) | ✓   | ✓   | parallel() + per-task mode pairs  | keep    |
| `fix` + `then`                 | ✓   | ✓   | series() + write/check task pairs | keep    |
| `stages`                       | ✓   | ✓   | series-of-parallels nesting       | keep    |
| name wildcards (`"cargo.*"`)   | ✓   | —   | 2 names in A                      | cut     |

## Surface

| Primitive                                  | A   | B   | Deletes lines                       | Verdict |
| ------------------------------------------ | --- | --- | ----------------------------------- | ------- |
| `agents` (harnesses/preTool/postEdit/stop) | ✓   | ✓   | 5 defineTaskHook blocks (~120 l.)   | keep    |
| `timeout` per slot                         | ✓   | ✓   | per-binding duplication ×3          | keep    |
| guard: path-derived (implicit)             | ✓   | ✓   | guard-edit-paths + reference-repos  | keep    |
| guard: builtin `destructive-commands`      | ✓   | ✓   | 27-line hook block                  | keep    |
| guard: tool-backed (`{ guard, tool }`)     | ✓   | ✓   | would-be bespoke secret-scan hook   | keep    |
| `overrides` per harness                    | ✓   | —   | 1 claude worktree setting           | cut     |
| `git` preCommit + `files: "staged"` + `also` | ✓ | ✓   | lefthook.yml                        | keep    |
| `git` commitMsg                            | ✓   | ✓   | lefthook commit-msg block           | keep    |
| `git` prePush                              | ✓   | ✓   | lefthook pre-push block             | keep    |
| `git` postCommit                           | ✓   | —   | lefthook post-commit block          | keep    |
| `ci` (github.pullRequest)                  | ✓   | ✓   | bespoke workflow steps              | keep    |
| `cli`                                      | ✓   | ✓   | package.json script indirection     | keep    |

`overrides` — ruled in Phase 4 (2026-06-10): **cut from user config,
relocated to sync internals**. The claude `worktreeSymlinkDirectories`
setting is harness runtime plumbing, not quality orchestration; it moves
alongside binary paths and cwd plumbing in `runweaver sync`. User config
stays pure: four orchestration concepts only.

## Resolved without a primitive

- **commit-msg file input**: trailing-argument convention (git's `$1`)
  appended by the commitMsg slot — a `{commitMsgFile}` placeholder was
  considered and not minted (one consumer).
- **post-commit install side effect**: `git.postCommit` ruled **keep**
  (2026-06-10, user arbitrage): the slot is a standard git hook stage and a
  production consumer is not speculative — same reasoning as `checkOnly`.
  The slot is generic; its content (an install script) is repo-specific
  script data, like any script entry.
- **conventional-commit on the agent surface**: dissolves into the git
  surface; Bash exit + stderr already reach the agent (hook 2, batch 1).
- **doc-drift declarative rules** (`{ when, expect }` pairs): policy-shaped,
  one consumer, no line savings over a script. Never drafted as a primitive.

## Tripwire report (hook 1, batch 1)

The guard kind set closed at three: **path-derived** (from Paths),
**tool-backed** (`{ guard: "secrets", tool: "gitleaks" }`), **builtin**
(`destructive-commands`). Batches 2-3 and config B were drafted without
needing a fourth kind and without needing any guard-only primitive that no
pipeline or CLI form can execute. **Tripwire NOT triggered.**

## Synthesis for Phase 4

The four concepts held across both configs: no fifth concept, no closure, no
new guard kind. Clean cuts: `denyWarnings`, `variants`, wildcards, `daemon`,
`config` (folds into `args`), `{commitMsgFile}` (never minted).

**Phase 4 ruling (2026-06-10): GO.** All four arbitrages resolved above:
`checkOnly` kept, declarative entries + parsers kept by charter (the one
deliberate criterion exception), `{files}`-in-scripts cut (first re-add
candidate), `overrides` relocated to sync internals. None was a concept
leak. The model survived its falsifiable test; Phase 5 builds against the
keep column of this document.
