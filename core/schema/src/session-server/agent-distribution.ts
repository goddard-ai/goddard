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
export const AgentDistributionEnv = z
  .record(z.string(), z.string())
  .describe("Environment variables to set when launching the distributed agent.")

export type AgentDistributionEnv = z.infer<typeof AgentDistributionEnv>

/** Binary execution target metadata for one supported platform. */
export const AgentBinaryTarget = z
  .object({
    archive: z.url().describe("Archive URL containing the platform-specific agent binary."),
    cmd: z.string().describe("Executable command to run after the archive is unpacked."),
    args: z
      .array(z.string())
      .optional()
      .describe("Default command-line arguments to pass to the binary."),
    env: AgentDistributionEnv.optional().describe(
      "Environment variables required by this binary target.",
    ),
  })
  .strict()
  .describe("Binary launch settings for one supported operating system and architecture.")

export type AgentBinaryTarget = z.infer<typeof AgentBinaryTarget>

/** Platform-indexed ACP binary targets. */
export const AgentBinaryDistribution = z
  .object(
    Object.fromEntries(
      agentBinaryPlatforms.map((platform) => [
        platform,
        AgentBinaryTarget.optional().describe(`Binary target for the ${platform} platform.`),
      ]),
    ) as Record<AgentBinaryPlatform, z.ZodOptional<typeof AgentBinaryTarget>>,
  )
  .strict()
  .refine((value) => Object.values(value).some((target) => target !== undefined), {
    message: "binary distributions must declare at least one supported platform",
  })
  .describe("Platform-specific binary targets for the agent.")

export type AgentBinaryDistribution = z.infer<typeof AgentBinaryDistribution>

/** Launch metadata for ACP package-based distributions. */
export const AgentPackageDistribution = z
  .object({
    package: z
      .string()
      .min(1)
      .describe("Package specifier to install or execute with the selected package runner."),
    args: z
      .array(z.string())
      .optional()
      .describe("Default command-line arguments to pass through the package runner."),
    env: AgentDistributionEnv.optional().describe(
      "Environment variables required by this package-based install method.",
    ),
  })
  .strict()
  .describe("Launch metadata for an agent distributed through a package runner.")

export type AgentPackageDistribution = z.infer<typeof AgentPackageDistribution>

/** Supported ACP distribution methods for one agent entry. */
export const AgentInstallationMethods = z
  .object({
    binary: AgentBinaryDistribution.optional().describe(
      "Binary artifacts to download and run directly.",
    ),
    npx: AgentPackageDistribution.optional().describe("Package distribution to run with npx."),
    uvx: AgentPackageDistribution.optional().describe("Package distribution to run with uvx."),
  })
  .strict()
  .refine((value) => Object.values(value).some((entry) => entry !== undefined), {
    message: "distribution must declare at least one install method",
  })
  .describe("Supported install methods for acquiring and launching the agent.")

export type AgentInstallationMethods = z.infer<typeof AgentInstallationMethods>

/** Structured ACP agent entry accepted by session creation APIs. */
export const AgentDistribution = z
  .object({
    id: z
      .string()
      .regex(/^[a-z][a-z0-9-]*$/)
      .describe("Stable machine-readable identifier for the agent."),
    name: z.string().min(1).describe("Human-readable agent name."),
    version: z
      .string()
      .regex(/^[0-9]+\.[0-9]+\.[0-9]+/)
      .describe("Semantic version of the agent distribution manifest."),
    description: z.string().min(1).describe("Short summary of what the agent does."),
    repository: z
      .url()
      .optional()
      .describe("Repository URL for the agent source or release metadata."),
    authors: z
      .array(z.string())
      .optional()
      .describe("List of authors or maintainers for the agent."),
    license: z.string().optional().describe("License identifier or label for the agent."),
    icon: z
      .string()
      .optional()
      .describe("Icon reference for the agent, such as an emoji or asset identifier."),
    distribution: AgentInstallationMethods.describe(
      "Instructions for installing and launching the agent.",
    ),
  })
  .strict()
  .describe(
    "Structured agent manifest accepted anywhere a config can inline an agent distribution.",
  )

export type AgentDistribution = z.infer<typeof AgentDistribution>
