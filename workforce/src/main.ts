#!/usr/bin/env node
import { cancel, intro, isCancel, multiselect, outro } from "@clack/prompts"
import { command, option, optional, positional, runSafely, string, subcommands } from "cmd-ts"
import { GoddardSdk } from "@goddard-ai/sdk/node"
import { execFile } from "node:child_process"
import { promisify } from "node:util"

const execFileAsync = promisify(execFile)

/** Workforce package metadata displayed during the CLI initialization flow. */
type WorkforceInitCandidate = {
  rootDir: string
  relativeDir: string
  manifestPath: string
  name: string
}

/** Creates one Node SDK instance for the provided daemon connection override. */
function getSdk(daemonUrl?: string): GoddardSdk {
  return new GoddardSdk({
    daemonUrl,
  })
}

/** Formats one discovered package for the workforce init selection prompt. */
export function formatPackageLabel(pkg: WorkforceInitCandidate): string {
  return pkg.relativeDir === "."
    ? `${pkg.name} (repository root)`
    : `${pkg.name} (${pkg.relativeDir})`
}

/** Resolves the effective request message from the flag or positional argument. */
export function resolveCommandMessage(input: {
  message?: string
  positionalMessage?: string
}): string {
  const message = input.message ?? input.positionalMessage
  if (message) {
    return message
  }

  throw new Error("A message is required. Pass --message or provide it positionally.")
}

/** Prompts the operator to choose which repository packages should become workforce domains. */
async function promptForWorkforcePackages(
  candidates: WorkforceInitCandidate[],
): Promise<string[] | null> {
  const selected = await multiselect({
    message: "Select packages to include as workforce domains.",
    options: candidates.map((candidate) => ({
      value: candidate.rootDir,
      label: formatPackageLabel(candidate),
      hint: candidate.relativeDir === "." ? "root agent" : candidate.relativeDir,
    })),
    initialValues: candidates.map((candidate) => candidate.rootDir),
  })

  if (isCancel(selected)) {
    cancel("Workforce initialization cancelled.")
    return null
  }

  return selected
}

/** Resolves the nearest git repository root from one starting directory. */
async function resolveRepositoryRoot(startDir: string): Promise<string> {
  try {
    const { stdout } = await execFileAsync("git", ["rev-parse", "--show-toplevel"], {
      cwd: startDir,
    })
    return stdout.trim()
  } catch (error) {
    throw new Error(
      `Unable to resolve the repository root from ${startDir}: ${
        error instanceof Error ? error.message : String(error)
      }`,
    )
  }
}

/** Runs the workforce CLI entrypoint against one argv payload. */
export async function main(argv: string[]) {
  const daemonUrl = option({
    type: optional(string),
    long: "daemon-url",
    description: "Daemon IPC URL to use instead of the environment default",
  })
  const root = option({
    type: string,
    long: "root",
    defaultValue: () => process.cwd(),
    description: "Repository root or any path inside the repository",
  })

  const app = subcommands({
    name: "goddard-workforce",
    description: "Manage daemon-owned workforce runtimes and requests",
    cmds: {
      init: command({
        name: "init",
        description: "Create repo-local workforce config and ledger files",
        args: { root, daemonUrl },
        handler: async ({ root, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const discovery = await sdk.workforce.discoverCandidates({
            rootDir: root,
          })

          if (discovery.candidates.length === 0) {
            console.log(`No workforce candidates found under ${discovery.rootDir}.`)
            return
          }

          intro(`Initializing workforce in ${discovery.rootDir}`)
          const selectedPackageDirs = await promptForWorkforcePackages(discovery.candidates)
          if (selectedPackageDirs === null) {
            return
          }

          const response = await sdk.workforce.initialize({
            rootDir: discovery.rootDir,
            packageDirs: selectedPackageDirs,
          })
          outro(`Initialized workforce config at ${response.initialized.configPath}.`)
        },
      }),
      start: command({
        name: "start",
        description: "Start a workforce runtime for one repository",
        args: { root, daemonUrl },
        handler: async ({ root, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(root)
          const response = await sdk.workforce.start({
            rootDir: repositoryRoot,
          })
          console.log(`Started workforce for ${response.workforce.rootDir}.`)
        },
      }),
      status: command({
        name: "status",
        description: "Show status for one workforce runtime",
        args: { root, daemonUrl },
        handler: async ({ root, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(root)
          const response = await sdk.workforce.get({
            rootDir: repositoryRoot,
          })
          console.log(JSON.stringify(response.workforce, null, 2))
        },
      }),
      list: command({
        name: "list",
        description: "List running workforce runtimes known to the daemon",
        args: { daemonUrl },
        handler: async ({ daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const response = await sdk.workforce.list({})
          console.log(JSON.stringify(response.workforces, null, 2))
        },
      }),
      request: command({
        name: "request",
        description: "Queue a new workforce request for a target agent",
        args: {
          root,
          daemonUrl,
          targetAgentId: option({
            type: string,
            long: "target-agent-id",
            short: "t",
            defaultValue: () => "root",
            description:
              "Target workforce agent id that should receive the request; defaults to root",
          }),
          message: option({
            type: optional(string),
            long: "message",
            description:
              "Request payload to send to the target workforce agent; overrides the positional message when both are provided",
          }),
          positionalMessage: positional({
            type: optional(string),
            displayName: "message",
            description: "Request payload to send to the target workforce agent",
          }),
        },
        handler: async (args) => {
          const sdk = getSdk(args.daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(args.root)
          const response = await sdk.workforce.request({
            rootDir: repositoryRoot,
            targetAgentId: args.targetAgentId,
            input: resolveCommandMessage(args),
          })
          console.log(`Queued workforce request ${response.requestId ?? "unknown"}.`)
        },
      }),
      create: command({
        name: "create",
        description: "Ask the root agent to scaffold a new project or add packages for a feature",
        args: {
          root,
          daemonUrl,
          message: option({
            type: optional(string),
            long: "message",
            description:
              "Feature request that may require creating a new project or new workspace packages; overrides the positional message when both are provided",
          }),
          positionalMessage: positional({
            type: optional(string),
            displayName: "message",
            description:
              "Feature request that may require creating a new project or new workspace packages",
          }),
        },
        handler: async (args) => {
          const sdk = getSdk(args.daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(args.root)
          const workforceResponse = await sdk.workforce.get({
            rootDir: repositoryRoot,
          })
          const response = await sdk.workforce.request({
            rootDir: repositoryRoot,
            targetAgentId: workforceResponse.workforce.config.rootAgentId,
            input: resolveCommandMessage(args),
            intent: "create",
          })
          console.log(`Queued create request ${response.requestId ?? "unknown"}.`)
        },
      }),
      update: command({
        name: "update",
        description: "Append updated input to an existing workforce request",
        args: {
          root,
          daemonUrl,
          requestId: option({
            type: string,
            long: "request-id",
            description: "Existing workforce request id to update",
          }),
          message: option({
            type: optional(string),
            long: "message",
            description:
              "Updated request payload to append; overrides the positional message when both are provided",
          }),
          positionalMessage: positional({
            type: optional(string),
            displayName: "message",
            description: "Updated request payload to append",
          }),
        },
        handler: async (args) => {
          const sdk = getSdk(args.daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(args.root)
          await sdk.workforce.update({
            rootDir: repositoryRoot,
            requestId: args.requestId,
            input: resolveCommandMessage(args),
          })
          console.log(`Updated workforce request ${args.requestId}.`)
        },
      }),
      cancel: command({
        name: "cancel",
        description: "Cancel an existing workforce request",
        args: {
          root,
          daemonUrl,
          requestId: option({
            type: string,
            long: "request-id",
            description: "Existing workforce request id to cancel",
          }),
          reason: option({
            type: optional(string),
            long: "reason",
            description: "Optional reason explaining why the request is being cancelled",
          }),
        },
        handler: async ({ root, requestId, reason, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(root)
          await sdk.workforce.cancel({
            rootDir: repositoryRoot,
            requestId,
            reason,
          })
          console.log(`Cancelled workforce request ${requestId}.`)
        },
      }),
      truncate: command({
        name: "truncate",
        description: "Clear pending work for a workforce scope",
        args: {
          root,
          daemonUrl,
          agentId: option({
            type: optional(string),
            long: "agent-id",
            description: "Optional workforce agent id whose pending work should be truncated",
          }),
          reason: option({
            type: optional(string),
            long: "reason",
            description: "Optional reason explaining why pending work is being truncated",
          }),
        },
        handler: async ({ root, agentId, reason, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(root)
          await sdk.workforce.truncate({
            rootDir: repositoryRoot,
            agentId,
            reason,
          })
          console.log(`Truncated workforce queue for ${repositoryRoot}.`)
        },
      }),
      stop: command({
        name: "stop",
        description: "Stop a running workforce runtime for one repository",
        args: { root, daemonUrl },
        handler: async ({ root, daemonUrl }) => {
          const sdk = getSdk(daemonUrl)
          const repositoryRoot = await resolveRepositoryRoot(root)
          await sdk.workforce.shutdown({
            rootDir: repositoryRoot,
          })
          console.log(`Stopped workforce for ${repositoryRoot}.`)
        },
      }),
    },
  })

  const result = await runSafely(app, argv)
  if (result._tag === "error") {
    if (result.error.config) {
      console.log(result.error.config.message)
    } else {
      const helpResult = await runSafely(app, ["--help"])
      if (helpResult._tag === "error" && helpResult.error.config) {
        console.log(helpResult.error.config.message)
      }
    }
    process.exit(1)
  }
}

if (import.meta.main) {
  await main(process.argv.slice(2))
}
