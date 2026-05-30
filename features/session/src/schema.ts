import { StaticSessionParams as StaticSessionParamsSchema } from "@goddard-ai/schema/config"
import { DaemonSessionId, DaemonSessionIdParams } from "@goddard-ai/schema/id"
import { textModelConfigSchema, type ModelConfig } from "ai-sdk-json-schema"
import { z } from "zod"

export { StaticSessionParamsSchema as StaticSessionParams }

/** Schema for one custom worktree plugin loaded from a filesystem path. */
export const WorktreePluginPathReference = z
  .strictObject({
    type: z.literal("path"),
    path: z
      .string()
      .min(1)
      .describe(
        "Absolute plugin module path or a path resolved relative to the Goddard global directory.",
      ),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a module path.")

export type WorktreePluginPathReference = z.infer<typeof WorktreePluginPathReference>

/** Schema for one custom worktree plugin loaded from a globally installed package. */
export const WorktreePluginPackageReference = z
  .strictObject({
    type: z.literal("package"),
    package: z
      .string()
      .min(1)
      .describe("Package name for a globally installed worktree plugin module."),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a globally installed package.")

export type WorktreePluginPackageReference = z.infer<typeof WorktreePluginPackageReference>

/** Schema for one custom worktree plugin reference declared in root config. */
export const WorktreePluginReference = z
  .discriminatedUnion("type", [WorktreePluginPathReference, WorktreePluginPackageReference])
  .describe("Custom worktree plugin reference loaded by the daemon from global config.")

export type WorktreePluginReference = z.infer<typeof WorktreePluginReference>

/** Schema for supported package managers used by daemon-managed worktree bootstrap. */
export const WorktreeBootstrapPackageManager = z
  .enum(["bun", "pnpm", "npm", "yarn"])
  .describe("Package manager command used to prepare fresh daemon-managed worktrees.")

export type WorktreeBootstrapPackageManager = z.infer<typeof WorktreeBootstrapPackageManager>

/** Schema for daemon-managed bootstrap defaults applied to fresh worktrees. */
export const WorktreeBootstrapConfig = z
  .strictObject({
    enabled: z
      .boolean()
      .optional()
      .describe("Whether daemon-managed worktree seeding and bootstrap are enabled."),
    packageManager: WorktreeBootstrapPackageManager.optional().describe(
      "Package manager command to run when bootstrapping a fresh daemon-managed worktree.",
    ),
    installArgs: z
      .array(z.string().min(1))
      .optional()
      .describe("Additional arguments appended to the selected package-manager install command."),
    seedEnabled: z
      .boolean()
      .optional()
      .describe("Whether selected untracked artifacts should be copied into fresh worktrees."),
    seedNames: z
      .array(z.string().min(1))
      .optional()
      .describe("Recursive basename allowlist used when selecting untracked seed candidates."),
    seedPaths: z
      .array(z.string().min(1))
      .optional()
      .describe("Exact repository-relative paths added to the untracked seed candidate set."),
  })
  .describe("Daemon-managed preparation settings applied to fresh worktrees.")

export type WorktreeBootstrapConfig = z.infer<typeof WorktreeBootstrapConfig>

/** Schema for persisted daemon worktree defaults loaded from JSON. */
export const WorktreesConfig = z
  .strictObject({
    defaultFolder: z
      .string()
      .min(1)
      .optional()
      .describe("Default repository-local folder name used for daemon-managed worktrees."),
    bootstrap: WorktreeBootstrapConfig.optional().describe(
      "Daemon-managed preparation defaults applied to fresh worktrees.",
    ),
    plugins: z
      .array(WorktreePluginReference)
      .optional()
      .describe("Custom worktree plugins loaded from the global Goddard config only."),
  })
  .describe("Persisted worktree defaults loaded from JSON.")

export type WorktreesConfig = z.infer<typeof WorktreesConfig>

/** Persisted session-title generation defaults loaded from JSON. */
export type SessionTitlesConfig = {
  generator?: ModelConfig
}

/** Schema for persisted session-title generation defaults loaded from JSON. */
export const SessionTitlesConfig: z.ZodType<SessionTitlesConfig> = z
  .strictObject({
    generator: textModelConfigSchema
      .optional()
      .describe("Text model selection used for background session title generation."),
  })
  .describe("Persisted session title-generation defaults loaded from JSON.")

/** Schema for package-boundary discovery settings used by the launch-session flow. */
export const SubpackagesConfig = z
  .strictObject({
    manifests: z
      .array(z.string().min(1))
      .optional()
      .describe(
        "Additional manifest filenames or relative manifest paths that mark subpackage directories.",
      ),
  })
  .describe("Persisted subpackage discovery settings loaded from JSON.")

export type SubpackagesConfig = z.infer<typeof SubpackagesConfig>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Response payload fragment returned after one daemon-managed session worktree fetch. */
export const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
})

export type SessionWorktree = z.infer<typeof SessionWorktree>

/** Session identity fragment shared by worktree responses. */
export type SessionWorktreeIdentity = {
  id: DaemonSessionId
  acpSessionId: string
}

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetSessionWorktreeResponse = SessionWorktreeIdentity & {
  worktree: SessionWorktree | null
}

/** Request payload used to read one daemon-managed session worktree. */
export const GetSessionWorktreeRequest = DaemonSessionIdParams

export type GetSessionWorktreeRequest = z.infer<typeof GetSessionWorktreeRequest>
