// Generated from the Runweaver manifest JSON schema. Do not edit.
// Regenerate with: runweaver manifest types
// schema-sha256: cfdfb184730a8118b89b47cbb90f693828ca73f2b6f2589d411e1afef695fe33

export type RunweaverOperationDefinitionManifest = {
  builtin: string;
  description?: string | null;
  kind: "operation";
  [k: string]: unknown;
};
export type PipelineDefinitionManifest =
  | {
      check: string[];
      [k: string]: unknown;
    }
  | {
      fix: string[];
      then?: PipelineDefinitionManifest | null;
      [k: string]: unknown;
    }
  | {
      stages: string[];
      [k: string]: unknown;
    };
export type AgentsPreToolGuardManifest =
  | {
      guard: AgentsBuiltinGuardManifest;
      [k: string]: unknown;
    }
  | {
      guard: string;
      tool: string;
      [k: string]: unknown;
    };
export type AgentsBuiltinGuardManifest = "destructive-commands";
export type GitFilesScopeManifest = "staged";
export type ToolDefinitionManifest =
  | {
      affected?: string[];
      args?: string[];
      preset: string;
      targets?: ToolTargetsManifest | null;
      [k: string]: unknown;
    }
  | {
      affected?: string[];
      check: string[];
      diagnostics: DiagnosticsParserManifest;
      fix?: string[] | null;
      targets?: ToolTargetsManifest | null;
      [k: string]: unknown;
    }
  | {
      script: string;
      [k: string]: unknown;
    };
export type ToolTargetsManifest = string[] | FileTargetsManifest;
export type DiagnosticsParserManifest =
  | {
      parser: NamedDiagnosticsParserManifest;
      [k: string]: unknown;
    }
  | {
      parser: NamedDiagnosticsParserManifest;
      pattern: string;
      [k: string]: unknown;
    };
export type NamedDiagnosticsParserManifest = "unix" | "regex";

/**
 * The serializable, closure-free form of a definition: path zones, tools, pipelines, operations, surfaces, and bindings as pure data. Executable behavior is referenced by builtin name and supplied at load time by a [`BuiltinRegistry`]. The JSON Schema for this type is exported via [`runweaver_manifest_json_schema`].
 */
export interface RunweaverDefinitionManifest {
  bindings: BindingManifest[];
  operations: {
    [k: string]: RunweaverOperationDefinitionManifest;
  };
  paths?: PathZonesManifest | null;
  pipelines: {
    [k: string]: PipelineDefinitionManifest;
  };
  surfaces?: SurfacesManifest | null;
  tools: {
    [k: string]: ToolDefinitionManifest;
  };
  version: number;
  [k: string]: unknown;
}
export interface BindingManifest {
  operationName: string;
  profiles?: ProfileManifest[];
  trigger: SurfaceTrigger;
  [k: string]: unknown;
}
export interface ProfileManifest {
  afterOperation: boolean;
  beforeOperation: boolean;
  builtin?: string | null;
  name: string;
  onOperationError: boolean;
  [k: string]: unknown;
}
/**
 * Identifies one event source: a surface name (e.g. `"agent-hook"`), a trigger name (e.g. `"post-edit"`), and an optional phase. Bindings match triggers exactly, including the phase.
 */
export interface SurfaceTrigger {
  name: string;
  phase?: string | null;
  surface: string;
  [k: string]: unknown;
}
/**
 * Path zones used by Runweaver surfaces. Each entry is a repository-relative path. Entries ending with / are prefix zones; other entries are exact file paths. Leading ./ is ignored and path separators normalize to /. Absolute paths inside the current hook cwd are normalized back to repository-relative paths before matching.
 */
export interface PathZonesManifest {
  checkOnly?: string[];
  generated?: string[];
  readOnly?: string[];
  writable?: string[];
  [k: string]: unknown;
}
export interface SurfacesManifest {
  agents?: AgentsSurfaceManifest | null;
  ci?: CiSurfaceManifest | null;
  cli?: boolean | null;
  git?: GitSurfaceManifest | null;
  [k: string]: unknown;
}
export interface AgentsSurfaceManifest {
  harnesses: string[];
  postEdit?: AgentsPipelineSlotManifest | null;
  preTool?: AgentsPreToolGuardManifest[];
  stop?: AgentsPipelineSlotManifest | null;
  [k: string]: unknown;
}
export interface AgentsPipelineSlotManifest {
  run: string;
  timeout?: number | null;
  [k: string]: unknown;
}
export interface CiSurfaceManifest {
  github?: GithubCiSurfaceManifest | null;
  [k: string]: unknown;
}
export interface GithubCiSurfaceManifest {
  pullRequest?: string | null;
  [k: string]: unknown;
}
export interface GitSurfaceManifest {
  /**
   * Directory of pre-existing hooks to chain after the generated hooks (repo-relative, e.g. ".githooks"). Each generated hook runs its runweaver slot first, then execs `<chainHooksDir>/<slot>` when present and executable.
   */
  chainHooksDir?: string | null;
  commitMsg?: GitToolSlotManifest | null;
  postCommit?: GitToolSlotManifest | null;
  preCommit?: GitPreCommitSlotManifest | null;
  prePush?: GitPipelineSlotManifest | null;
  [k: string]: unknown;
}
export interface GitToolSlotManifest {
  tool: string;
  [k: string]: unknown;
}
export interface GitPreCommitSlotManifest {
  also?: string[];
  files?: GitFilesScopeManifest | null;
  run: string;
  [k: string]: unknown;
}
export interface GitPipelineSlotManifest {
  run: string;
  [k: string]: unknown;
}
export interface FileTargetsManifest {
  extensions?: string[];
  /**
   * File arguments used when a file-scoped tool runs without explicit files.
   */
  fallback?: string[];
  files?: string[];
  prefixes?: string[];
  [k: string]: unknown;
}
