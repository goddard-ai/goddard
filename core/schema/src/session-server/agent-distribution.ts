import { z } from "zod"

/** Supported platform keys for ACP binary distributions. */
export const agentBinaryPlatforms = [
  "darwin-aarch64",
  "darwin-x86_64",
  "linux-aarch64",
  "linux-x86_64",
  "windows-aarch64",
  "windows-x86_64",
] as const

/** Supported platform keys for ACP binary distributions. */
export type AgentBinaryPlatform = (typeof agentBinaryPlatforms)[number]

/** Environment variables declared by an ACP distribution target. */
export const AgentDistributionEnv = z.record(z.string(), z.string())

export type AgentDistributionEnv = z.infer<typeof AgentDistributionEnv>

/** Binary execution target metadata for one supported platform. `archive` may be zip, tar.gz, tgz, tar.bz2, tbz2, or a raw binary payload, and `cmd` resolves from the installed root. */
export const AgentBinaryTarget = z
  .object({
    archive: z.url(),
    cmd: z.string(),
    args: z.array(z.string()).optional(),
    env: AgentDistributionEnv.optional(),
  })
  .strict()

export type AgentBinaryTarget = z.infer<typeof AgentBinaryTarget>

/** Platform-indexed ACP binary targets. */
export const AgentBinaryDistribution = z
  .object(
    Object.fromEntries(
      agentBinaryPlatforms.map((platform) => [platform, AgentBinaryTarget.optional()]),
    ) as Record<AgentBinaryPlatform, z.ZodOptional<typeof AgentBinaryTarget>>,
  )
  .strict()
  .refine((value) => Object.values(value).some((target) => target !== undefined), {
    message: "binary distributions must declare at least one supported platform",
  })

export type AgentBinaryDistribution = z.infer<typeof AgentBinaryDistribution>

/** Launch metadata for ACP package-based distributions. */
export const AgentPackageDistribution = z
  .object({
    package: z.string().min(1),
    args: z.array(z.string()).optional(),
    env: AgentDistributionEnv.optional(),
  })
  .strict()

export type AgentPackageDistribution = z.infer<typeof AgentPackageDistribution>

/** Supported ACP distribution methods for one agent entry. */
export const AgentInstallationMethods = z
  .object({
    binary: AgentBinaryDistribution.optional(),
    npx: AgentPackageDistribution.optional(),
    uvx: AgentPackageDistribution.optional(),
  })
  .strict()
  .refine((value) => Object.values(value).some((entry) => entry !== undefined), {
    message: "distribution must declare at least one install method",
  })

export type AgentInstallationMethods = z.infer<typeof AgentInstallationMethods>

/** Structured ACP agent entry accepted by session creation APIs. */
export const AgentDistribution = z
  .object({
    id: z.string().regex(/^[a-z][a-z0-9-]*$/),
    name: z.string().min(1),
    version: z.string().regex(/^[0-9]+\.[0-9]+\.[0-9]+/),
    description: z.string().min(1),
    repository: z.url().optional(),
    authors: z.array(z.string()).optional(),
    license: z.string().optional(),
    icon: z.string().optional(),
    distribution: AgentInstallationMethods,
  })
  .strict()

export type AgentDistribution = z.infer<typeof AgentDistribution>
