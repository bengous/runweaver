// @ts-nocheck — design artifact: the import below does not exist yet.
/**
 * FUTURE HOOKS — Phase 3 design artifact, NOT executable.
 *
 * Dream configs for the next hooks of THIS repo, written against the
 * 4-concept model of `target-config.example.ts` (Paths, Tool, Pipeline,
 * Surface) in its data-literal form (no constructors; types generated from
 * the Rust schema). Each section is a delta to that file: only the entries
 * that change are shown. Constraints inherited from the design contract:
 *
 *   - closure-free / fully serializable (placeholders + parser specs)
 *   - exactly four user-facing concepts; a needed 5th is a FINDING, not
 *     a license to invent
 *   - tools are never embedded; preset -> declarative entry -> script ladder
 *
 * The 5 existing hooks (guard-destructive, guard-edit-paths,
 * guard-reference-repos, post-edit-quality, stop-validate) are already
 * re-expressed in `target-config.example.ts` (agents surface block) and
 * count toward the Phase 3 tally there.
 *
 * Status: batches 1-3 (hooks 1-10). Batch 1 (hooks 1-3) ratified 2026-06-10.
 * Batches 2-3 pending review. FINDINGS at each section are the experimental
 * data for PRIMITIVES.md and the Phase 4 go/no-go.
 */

import { defineRunweaver } from "@bengous/runweaver";

// ── Hook 1: secret-scan pre-tool ────────────────────────────────────────────
// Want: block an Edit/Write/Bash tool call whose PENDING content contains a
// secret, before it lands on disk. The natural spelling reuses the gitleaks
// tool entry already bound on git pre-commit — same Tool, different Surface:
//
//   surfaces: {
//     agents: {
//       harnesses: ["pi", "codex", "claude"],
//       preTool: [
//         { guard: "destructive-commands" },
//         { guard: "secrets", tool: "gitleaks" },
//       ],
//       ...
//     },
//     git: {
//       preCommit: { run: "autofix", files: "staged", also: ["gitleaks"] },
//       ...
//     },
//   }
//
// Semantics: the agents surface feeds the pending tool-call content (new file
// text, command string) to gitleaks in pipe mode; fail(diagnostics) projects
// as block-with-reason on every harness that supports pre-tool blocking.
// No new pipeline: a guard runs a single tool in check mode on
// surface-scoped input.
//
// FINDING (concept pressure): `guard` entries are quietly becoming a slot
// for arbitrary pre-tool checks, not just path-derived ones. Two readings:
//   (a) a guard IS a Tool in check mode whose input scoping (pending content
//       instead of files on disk) is Surface codec knowledge — 4 concepts
//       hold; `{ guard: "secrets", tool: "gitleaks" }` names the scoping
//       and the tool explicitly;
//   (b) guards are a 5th concept (pre-tool predicates with their own
//       contract). If batch 2+ keeps needing guard-only primitives that no
//       pipeline can run, reading (b) wins and that is a no-go signal.
// Drafted under reading (a). Watch it.

// ── Hook 2: conventional-commit check on the agent surface ─────────────────
// Want: when the agent runs `git commit -m "..."`, validate the message
// against commitlint BEFORE the commit, so the agent gets block-with-reason
// instead of a failed Bash call.
//
// Honest draft: THIS HOOK DISSOLVES. The git surface already owns it:
//
//   git: {
//     commitMsg: { tool: "commitlint" },
//     ...
//   }
//
// The commit-msg git hook fires on agent-initiated commits too (the agent
// shells out to `git commit`), and its abort + diagnostics reach the agent
// through the Bash tool result. An agent-surface duplicate would require the
// Surface to parse `-m`/`-F`/heredoc out of an arbitrary command string —
// fragile codec knowledge with no payoff over the git-hook path.
//
// FINDING (positive): a candidate hook that dissolves into an existing
// surface binding is the model working as intended — outcome projection
// through Bash exit + stderr is already agent-readable. Cost: one extra
// failed `git commit` round-trip versus a true pre-tool block. Accepted.
// No primitive is added; the commitlint entry gets a second real consumer
// through the second example config via the git surface only.

// ── Hook 3: affected-tests post-edit ────────────────────────────────────────
// Want: after the agent edits source files, run only the tests affected by
// those files, inside the post-edit latency budget. The mapping edited-file
// -> test-file is Tool knowledge (it describes how the test runner targets
// files), declared as serializable placeholder patterns, never a function:
//
//   tools: {
//     test: {
//       preset: "bun-test",
//       targets: [".runweaver/project-specific", "harness", "platform"],
//       affected: ["{dir}/{stem}.test.{ext}", "{dir}/*.test.ts"],
//     },
//   },
//
//   pipelines: {
//     // autofix grows a test stage: fixes write first, then checks confirm.
//     autofix: { fix: ["oxlint", "oxfmt", "cargo.fmt"], then: { check: ["oxlint", "test"] } },
//   },
//
// Scoping semantics: when a pipeline runs file-scoped (post-edit gives the
// surface the touched paths), `test` resolves each touched file through its
// `affected` patterns and runs the union; unscoped runs (stop, CI, CLI)
// ignore `affected` and run full `targets`. Empty resolution -> the standard
// automatic empty-scope skip. Same tool, same pipeline word, scope decides.
//
// FINDING (latency risk): bun test on one directory fits the 90 s post-edit
// timeout today, but this is the first hook whose cost scales with the
// repo's test suite shape, not with the diff. If it breaches the budget the
// fallback is moving `test` from `autofix` (post-edit) to `validate` (stop)
// — a 1-line pipeline edit, which is the model behaving well.
// FINDING (primitive): `affected` placeholder patterns are a new Tool field.
// Keep only if the Python config independently wants it (pytest has no
// sibling-test convention by default; importlib-based selection would NOT be
// expressible as path patterns). To verify in the second config.

// ═══ BATCH 2 ════════════════════════════════════════════════════════════════

// ── Hook 4: doc-drift check on stop ─────────────────────────────────────────
// Want: at session stop, flag code zones touched during the session whose
// tracking docs were NOT touched (e.g. src/ changed but
// docs/architecture.md untouched; harness layout changed but README.md untouched).
// The drift logic (which doc tracks which zone, what counts as stale) is not
// declarative-shaped — it is repo policy. The ladder says that is a script:
//
//   tools: {
//     docDrift: {
//       script: "bun .runweaver/project-specific/checks/doc-drift.ts {files}",
//     },
//   },
//
//   pipelines: {
//     audits: { check: ["pathChecks", "lintAudit", "docDrift"] },
//   },
//
// Stop already runs `validate` (which stages `audits`) scoped to the
// session's touched paths, so `{files}` here receives the session file list
// — the differentiator (session state) consumed by a plain script. Unscoped
// runs (CI, CLI) pass the full git diff against the default branch.
// Non-zero exit -> fail(diagnostics from output) -> pi blocks the stop,
// CI annotates.
//
// FINDING (ladder works): a declarative alternative was considered and
// rejected — `drift: [{ when: "src/**", expect: "docs/architecture.md" }]` as a
// new Tool field. One consumer, fuzzy semantics (when is a doc "updated
// enough"?), and it deletes no lines versus the script. Textbook cut. The
// finding is that the script rung absorbs policy-shaped checks without
// pressuring the concept budget.
// FINDING (primitive): `{files}` as a placeholder in SCRIPT entries (not
// just declarative tool entries) — first consumer here. Needs a second one
// or it goes to the cut column. To verify in the Python config.

// ── Hook 5: architecture-boundary check post-edit ───────────────────────────
// Want: an agent that introduces a forbidden import (harness/extensions ->
// platform/, or platform/ -> .runweaver/project-specific/) hears about it at
// edit time, not minutes later at stop validation.
//
// Full dream config delta — ONE WORD:
//
//   pipelines: {
//     autofix: {
//       fix: ["oxlint", "oxfmt", "cargo.fmt"],
//       then: { check: ["oxlint", "test", "depcruise"] },
//     },
//   },
//
// `depcruise` already exists as a tool entry (staticAnalysis pipeline, stop +
// CI). dependency-cruiser accepts file arguments, so post-edit scoping is
// preset knowledge already required elsewhere; the boundary rules stay in
// .runweaver/configs/dependency-cruiser.cjs where they live today.
//
// FINDING (strongest positive of the batch): a brand-new hook costs zero new
// primitives and zero new tool entries — it is a recomposition. "Add a hook"
// degenerating into "add one word to a pipeline" is the model's best
// behavior so far.
// FINDING (latency): dependency-cruiser cold-starts in seconds (node). Fits
// the 90 s post-edit budget but burns agent-perceived latency; if it stings,
// the `daemon` slot sketched in target-config's biome comment is the
// designed escape — do NOT promote it to a primitive until a config actually
// writes it.

// ═══ BATCH 3 — the 5 existing hooks, re-expressed ═══════════════════════════
// These already appear in target-config.example.ts; this batch audits each
// one against a large existing config and records what the re-expression
// deletes.

// ── Hook 6: guard-destructive ───────────────────────────────────────────────
// Today: 27-line defineTaskHook block (3 harness bindings, matchers,
// timeouts, commandPrefixes) + task entry + runHookGuardDestructive gate.
// Target:
//
//   agents: { preTool: [{ guard: "destructive-commands" }] }
//
// FINDING (tripwire boundary, documented not dodged): this is the ONE guard
// that is neither derived from Paths nor backed by a Tool entry — it is a
// builtin of the Rust binary (command-string analysis). It is guard-only in
// practice; its pipeline-executability is limited to a CLI debugging form
// (`echo "rm -rf /" | runweaver guard destructive`). The guard kind set is
// hereby CLOSED at three: path-derived (from Paths), tool-backed
// ({ guard, tool }), builtin (this one). Any batch or config that needs a
// FOURTH kind triggers the hook-1 tripwire -> no-go signal, not a workaround.

// ── Hook 7: guard-edit-paths ────────────────────────────────────────────────
// Today: 27-line defineTaskHook block + 16 lines of zone constants + gate
// code + per-harness matcher duplication.
// Target: NOTHING. Derived entirely from `paths` (generated + readOnly +
// writable/checkOnly), bound automatically on every harness that supports
// pre-tool blocking.
//
// FINDING: the strongest primitive in the system — the Paths concept pays
// its whole rent here. Zero config lines for the hook itself; the zones are
// declared once and three other behaviors (fix→check degradation,
// empty-scope skip, this guard) reuse them.

// ── Hook 8: guard-reference-repos ───────────────────────────────────────────
// Today: 15-line pi-only defineTaskHook block + gate code.
// Target: NOTHING beyond `paths.readOnly: ["vendor/reference/"]`.
//
// FINDING: today's pi-only binding disappears — the target applies the guard
// uniformly wherever pre-tool blocking exists (open question 3 in
// target-config: verify nothing relied on the asymmetry before cutover).
// Subsumed by hook 7's mechanism; not a separate primitive.

// ── Hook 9: post-edit-quality ───────────────────────────────────────────────
// Today: 27-line defineTaskHook block + post-edit-quality.ts module
// (snapshot/feedback logic, ~300 lines) + the embedded-config wrapper.
// Target:
//
//   agents: { postEdit: { run: "autofix", timeout: 90 } }
//
// FINDING: the deletion is bought by the outcome contract, not by the hook
// slot — `fixed(changedFiles)` is what lets the claude codec own snapshot
// reinjection (updatedToolOutput) and the codex codec own block-with-reason.
// The user names a pipeline; the projection is codec knowledge. If a harness
// someday needs user-tunable projection, THAT would be a fifth-concept
// pressure point to document.

// ── Hook 10: stop-validate ──────────────────────────────────────────────────
// Today: 25-line defineTaskHook block + stop gating logic in hook-gates.ts
// (fingerprint, touched-path accumulation, relevance check).
// Target:
//
//   agents: { stop: { run: "validate", timeout: 240 } }
//
// FINDING: session semantics (touched-path accumulation, git-fingerprint
// read-only proof, run-only-after-relevant-changes) move wholesale into the
// agents surface — they are the differentiator, and they are NOT user
// config. Today's pi-only `runWhen: "after-relevant-changes"` becomes the
// default everywhere; per-harness capability gaps surface in the sync
// capability-loss report instead of in user-maintained bindings.

export const __designArtifact = true;
