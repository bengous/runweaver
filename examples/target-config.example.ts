// @ts-nocheck — design artifact: the import below does not exist yet.
/**
 * TARGET CONFIG — design artifact, NOT executable.
 *
 * This is the config `runweaver.config.ts` should become once the preset and
 * surface-projection layers exist. It covers a full project quality setup in
 * 4 user-facing concepts:
 *
 *   Paths     — write zones declared once; edit guards and fix→check
 *               degradation derive from them automatically
 *   Tool      — a preset that knows its check/fix modes, target file types,
 *               and how to parse its output into diagnostics
 *   Pipeline  — a composition of tools in a chosen mode
 *   Surface   — a consumer (agents, git, ci, cli) that scopes inputs and
 *               projects outcomes into its native protocol
 *
 * Form (decided 2026-06-10, Phase 3): the config is a single DATA LITERAL.
 * `defineRunweaver` is an identity function whose parameter type is GENERATED
 * from the Rust binary's schema (schemars -> JSON Schema -> .d.ts). The TS
 * package ships no constructors and no logic — the Rust schema is the single
 * source of truth, there is no hand-maintained TS mirror. TS buys editor
 * autocompletion plus sync-time constants/spreads, nothing else.
 *
 * The load-bearing internal contract (users never write it):
 *
 *   every pipeline run produces   pass | fail(diagnostics) | fixed(changedFiles)
 *
 * and each surface projects that outcome:
 *
 *   claude post-edit  -> updatedToolOutput snapshot of the reformatted file
 *   codex  pre-tool   -> block with reason
 *   custom stop       -> block session until validation passes
 *   git    pre-commit -> abort commit (fixed files re-staged in fix mode)
 *   ci     PR         -> failing check with inline annotations
 *   cli               -> exit code + human-readable report
 *
 * Leanness rule this file enforces: a primitive earns its existence only if
 * it deletes lines from a real config. See the appendix at the bottom for
 * the mapping from today's config to this one.
 */

import { defineRunweaver } from "@bengous/runweaver";

export default defineRunweaver({
  // ── Paths: write zones, declared once ────────────────────────────────────
  // Everything derives from this block:
  //  - fix-mode tools write only inside `writable`; on `checkOnly` paths the
  //    same tool silently degrades to check mode (today: formatWriteWritable
  //    + formatCheckOnly + 3 fileTargetPolicy gates)
  //  - `generated` and `readOnly` produce agent edit guards automatically
  //    (today: hook:guard-edit-paths + hook:guard-reference-repos + bindings)
  //  - a tool whose resolved target set is empty skips with a reason
  //    (today: the 5 hand-written `has*TargetFiles` policies)
  paths: {
    writable: [
      "src/",
      "harness/",
      "platform/",
      "crates/",
      ".runweaver/project-specific/",
      ".claude/",
      ".custom/",
      ".agents/",
      "runweaver.config.ts",
    ],
    checkOnly: [".codex/"],
    generated: ["settings.managed.json", ".custom/hooks.jsonc", ".codex/config.toml"],
    readOnly: ["vendor/reference/"],
  },

  // ── Tools: presets, not tasks ─────────────────────────────────────────────
  // A tool is a named entry; `preset` pulls the binary's built-in knowledge
  // of that tool (target extensions, check vs fix invocation, output parsing
  // into diagnostics, file-scoping behavior). The check/fix mode is chosen
  // by the pipeline/surface, never duplicated as two entries. Preset
  // knowledge lives in the Rust binary — where the parsers already are —
  // not in a TS package.
  tools: {
    oxfmt: { preset: "oxfmt", config: ".runweaver/configs/oxfmtrc.jsonc" },
    oxlint: { preset: "oxlint", config: ".runweaver/configs/oxlintrc.jsonc" },
    tsc: { preset: "tsc" }, // whole-program; ignores file scoping by nature
    // fix mode maps touched *.rs -> `-p <package>` (today: 30-line helper)
    "cargo.fmt": { preset: "cargo-fmt" },
    "cargo.check": {
      preset: "cargo-check",
      args: ["--workspace", "--all-targets", "--all-features", "--locked"],
    },
    "cargo.clippy": { preset: "cargo-clippy", denyWarnings: true },
    depcruise: {
      preset: "dependency-cruiser",
      config: ".runweaver/configs/dependency-cruiser.cjs",
      targets: [".runweaver/project-specific", "runweaver.config.ts", "harness", "platform"],
    },
    knip: { preset: "knip", config: ".runweaver/configs/knip.jsonc" },
    jscpd: { preset: "jscpd", config: ".runweaver/configs/jscpd.json" },
    test: {
      preset: "bun-test",
      targets: [".runweaver/project-specific", "harness", "platform"],
    },
    gitleaks: { preset: "gitleaks" },
    commitlint: { preset: "commitlint", config: ".runweaver/configs/commitlint.config.js" },

    // Tool-agnosticism is the contract: presets are just prepackaged
    // adapters, and the set is open. Any tool plugs in by declaring the same
    // contract — fully declaratively (placeholders and parser specs, never
    // functions: the config must stay serializable to the manifest). A
    // project using biome instead of oxlint/oxfmt writes:
    //
    //   biome: {
    //     targets: { extensions: ["ts", "tsx", "js", "jsx", "json"] },
    //     check: ["biome", "check", "{files}"],
    //     fix: ["biome", "check", "--write", "{files}"],
    //     diagnostics: { parser: "github" }, // or "unix" / "sarif" / { regex }
    //     // fast-invocation knowledge also fits the contract: the
    //     // orchestrator never embeds a tool, but it can know how to talk
    //     // to its warm form (eslint_d, biome daemon, tsserver):
    //     // daemon: { start: [...], check: [...] },
    //   },
    //
    // Escape hatch below that: repo-specific checks stay plain scripts. A
    // script is a check-only tool whose non-zero exit becomes
    // fail(diagnostics from output).
    managedSettings: { script: "bun platform/cli/managed-settings.ts check" },
    pathChecks: {
      script: "bun .runweaver/project-specific/checks/path-checks/cli.ts",
      variants: ["links", "code", "docs"],
    },
    lintAudit: { script: "bun .runweaver/project-specific/audits/audit-oxlint-rules.ts" },
    validatePush: { script: "bun .runweaver/project-specific/validation/validate-push.ts" },
    installAgentConfig: { script: "bun platform/cli/install-agent-config-after-commit.ts" },
    mcpUpdates: { script: "bun .runweaver/project-specific/audits/check-mcp-updates.ts" },
  },

  // ── Pipelines: compositions, mode chosen here ─────────────────────────────
  // `check` runs tools in parallel in check mode; `fix` runs them in
  // sequence in fix mode (writes must not race). `stages` is a series of
  // groups. Sync drift of generated surface configs is checked by the lib
  // itself (it owns those files), so today's `hooksCheck` task disappears.
  pipelines: {
    check: { check: ["oxfmt", "oxlint", "tsc", "cargo.*", "managedSettings", "test"] },
    staticAnalysis: { check: ["depcruise", "knip", "jscpd"] },
    audits: { check: ["pathChecks", "lintAudit"] },
    validate: { stages: ["check", "staticAnalysis", "audits"] },
    autofix: { fix: ["oxlint", "oxfmt", "cargo.fmt"], then: { check: ["oxlint"] } },
  },

  // ── Surfaces: consumers; scoping and projection live here ────────────────
  surfaces: {
    // The agents adapter owns, per harness:
    //  - native matcher translation: the user never writes "Bash" vs "^Bash$"
    //    vs "apply_patch|Edit|Write|MultiEdit" — those are codec knowledge
    //  - session state: touched paths accumulated across edits
    //  - stop semantics: validation scoped to session touched paths, proved
    //    read-only via git fingerprint, runs only after relevant changes
    //  - emission of .custom/hooks.jsonc, .codex/config.toml, .claude/settings.json
    //    via `runweaver sync`, with a per-harness capability loss report
    //  - binary path / cwd env-var plumbing (today: 3 commandPrefix constants)
    // Implicit, derived from `paths`: edit-zone guards on every harness that
    // supports pre-tool blocking.
    agents: {
      harnesses: ["custom", "codex", "claude"],
      preTool: [{ guard: "destructive-commands" }],
      postEdit: { run: "autofix", timeout: 90 },
      stop: { run: "validate", timeout: 240 },
      overrides: {
        claude: { worktreeSymlinkDirectories: ["node_modules"] },
      },
    },

    // The git adapter replaces lefthook.yml: same pipelines, staged-file
    // scoping, fixed files re-staged on pre-commit fix.
    git: {
      preCommit: { run: "autofix", files: "staged", also: ["gitleaks"] },
      commitMsg: { tool: "commitlint" },
      prePush: { run: "validatePush" },
      // ⚠ single consumer — pending user arbitrage.
      postCommit: { tool: "installAgentConfig" },
    },

    // Same `validate` pipeline, projected as PR annotations.
    ci: { github: { pullRequest: "validate" } },

    // Exposes `runweaver run <pipeline|tool>` — replaces the package.json
    // script indirection (`bun run check` -> cargo run -- run check).
    // Standalone scripts (mcpUpdates) stay reachable here without belonging
    // to any pipeline.
    cli: true,
  },
});

/**
 * ── Appendix: mapping from today's config ─────────────────────────────────
 *
 * | Today (486 lines + project-specific gates)            | Target                          |
 * |-------------------------------------------------------|---------------------------------|
 * | 5 fileTargetPolicy gates                               | automatic empty-scope skip      |
 * | formatCheck/formatWriteWritable/formatCheckOnly/...    | oxfmt entry + paths zones       |
 * | lintErrors/lintFiles/lintFix (3 tasks)                 | oxlint entry                    |
 * | cargoFmtCheck/cargoFmtWrite + cargoFmtWriteArgs helper | cargo.fmt entry                 |
 * | hook:guard-edit-paths + hook:guard-reference-repos     | derived from paths zones        |
 * |   + gate code + per-harness bindings                   |                                 |
 * | hook:post-edit-quality + post-edit-quality.ts          | postEdit: "autofix" + outcome   |
 * |   (snapshot/feedback logic)                            |   projection in claude codec    |
 * | hook:stop-validation + hook-gates.ts stop logic        | stop: "validate" + agents       |
 * |   (fingerprint, touched paths, session state)          |   surface session semantics     |
 * | 5 defineTaskHook blocks with matchers/timeouts/        | agents surface defaults;        |
 * |   commandPrefixes per harness (~120 lines)             |   matchers are codec knowledge  |
 * | runweaverHookCommand / binary path / cwd plumbing       | `runweaver sync` internals       |
 * | hooksCheck task                                        | built-in sync drift check       |
 * | lefthook.yml                                           | git surface                     |
 * | package.json script indirection                        | cli surface                     |
 *
 * Still user-owned: tool config file paths, repo-specific scripts, pipeline
 * composition, zone declarations, timeouts.
 *
 * Deliberately dropped (manual-only today, no pipeline consumes them):
 * checkFormatDrift. Re-add only when a surface needs it.
 *
 * ── Open design questions ─────────────────────────────────────────────────
 *
 * 1. Preset maintenance: each preset encodes a tool's CLI. Tool releases can
 *    break presets — that is the library's burden forever. Mitigation: keep
 *    presets thin (args assembly + exit-code rules + output parsing), version
 *    them against tool major versions. Presets live in the Rust binary, so
 *    the schema (and the generated TS types) always match what the binary
 *    can actually run.
 * 2. Escape-hatch ladder: preset -> declarative tool entry (placeholders +
 *    parser specs) -> script (opaque subprocess). The middle rung is what
 *    keeps the lib tool-agnostic: presets are conveniences, not privileges.
 *    Every rung is pure data — the config serializes to the manifest as-is.
 * 3. Uniform guards vs today's asymmetry: current config binds edit guards on
 *    two harnesses only and reference-repo guards on one only. Target derives
 *    guards from zones and applies them wherever the harness supports
 *    pre-tool blocking. The asymmetry is treated as accidental complexity —
 *    verify nothing relied on it before cutover.
 * 4. (Dissolved 2026-06-10 by the data-literal form.) `check` the pipeline
 *    name and `check` the mode key coexist as `check: { check: [...] }` —
 *    reads fine, no constructor to rename.
 * 5. Schema codegen: the .d.ts is generated at binary build time
 *    (schemars -> JSON Schema -> codegen). Adopting projects need node only
 *    at sync time, never at hook time. Accepted 2026-06-10.
 */
