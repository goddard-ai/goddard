#!/usr/bin/env node
import { cancel, intro, isCancel, multiselect, outro } from "@clack/prompts"
import {
  discoverWorkforceInitCandidates,
  initializeWorkforce,
  resolveRepositoryRoot,
  type DiscoveredWorkforcePackage,
} from "@goddard-ai/daemon/workforce"
import { command, option, optional, positional, runSafely, string, subcommands } from "cmd-ts"
import {
  cancelWorkforceRequest,
  createWorkforceRequest,
  getWorkforce,
  listWorkforces,
  startWorkforce,
  stopWorkforce,
  truncateWorkforce,
  updateWorkforceRequest,
} from "@goddard-ai/sdk/node"

/** Formats one discovered package for the workforce init selection prompt. */
export function formatPackageLabel(pkg: DiscoveredWorkforcePackage): string {
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
export async function promptForWorkforcePackages(
  candidates: DiscoveredWorkforcePackage[],
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

/** Initializes repo-local workforce files after resolving the repository root and selected domains. */
export async function runInitCommand(args: { root: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const candidates = await discoverWorkforceInitCandidates(repositoryRoot)

  if (candidates.length === 0) {
    console.log(`No workforce candidates found under ${repositoryRoot}.`)
    return
  }

  intro(`Initializing workforce in ${repositoryRoot}`)
  const selectedPackageDirs = await promptForWorkforcePackages(candidates)
  if (selectedPackageDirs === null) {
    return
  }

  const initialized = await initializeWorkforce(repositoryRoot, selectedPackageDirs)
  outro(`Initialized workforce config at ${initialized.configPath}.`)
}

/** Starts the daemon-managed workforce runtime for the resolved repository root. */
export async function runStartCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const workforce = await startWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(`Started workforce for ${workforce.rootDir}.`)
}

/** Prints the current daemon-managed workforce state for the resolved repository root. */
export async function runStatusCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const workforce = await getWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(JSON.stringify(workforce, null, 2))
}

/** Lists all daemon-managed workforce runtimes currently known to the daemon. */
export async function runListCommand(args: { daemonUrl?: string }): Promise<void> {
  const workforces = await listWorkforces({
    daemonUrl: args.daemonUrl,
  })
  console.log(JSON.stringify(workforces, null, 2))
}

/** Queues one workforce request for an explicit target agent. */
export async function runRequestCommand(args: {
  root: string
  targetAgentId: string
  message?: string
  positionalMessage?: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const response = await createWorkforceRequest(
    {
      rootDir: repositoryRoot,
      targetAgentId: args.targetAgentId,
      message: resolveCommandMessage(args),
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Queued workforce request ${response.requestId ?? "unknown"}.`)
}

/** Queues one create-intent request for the repository root agent. */
export async function runCreateCommand(args: {
  root: string
  message?: string
  positionalMessage?: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const workforce = await getWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  const response = await createWorkforceRequest(
    {
      rootDir: repositoryRoot,
      targetAgentId: workforce.config.rootAgentId,
      message: resolveCommandMessage(args),
      intent: "create",
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Queued create request ${response.requestId ?? "unknown"}.`)
}

/** Updates the user-facing input on one queued or suspended workforce request. */
export async function runUpdateCommand(args: {
  root: string
  requestId: string
  message?: string
  positionalMessage?: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await updateWorkforceRequest(
    {
      rootDir: repositoryRoot,
      requestId: args.requestId,
      message: resolveCommandMessage(args),
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Updated workforce request ${args.requestId}.`)
}

/** Cancels one existing workforce request. */
export async function runCancelCommand(args: {
  root: string
  requestId: string
  reason?: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await cancelWorkforceRequest(
    {
      rootDir: repositoryRoot,
      requestId: args.requestId,
      reason: args.reason,
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Cancelled workforce request ${args.requestId}.`)
}

/** Truncates pending workforce work for one repository or one agent queue. */
export async function runTruncateCommand(args: {
  root: string
  agentId?: string
  reason?: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await truncateWorkforce(
    {
      rootDir: repositoryRoot,
      agentId: args.agentId,
      reason: args.reason,
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Truncated workforce queue for ${repositoryRoot}.`)
}

/** Stops the daemon-managed workforce runtime for the resolved repository root. */
export async function runStopCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await stopWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(`Stopped workforce for ${repositoryRoot}.`)
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
        args: { root },
        handler: runInitCommand,
      }),
      start: command({
        name: "start",
        description: "Start a workforce runtime for one repository",
        args: { root, daemonUrl },
        handler: runStartCommand,
      }),
      status: command({
        name: "status",
        description: "Show status for one workforce runtime",
        args: { root, daemonUrl },
        handler: runStatusCommand,
      }),
      list: command({
        name: "list",
        description: "List running workforce runtimes known to the daemon",
        args: { daemonUrl },
        handler: runListCommand,
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
        handler: runRequestCommand,
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
        handler: runCreateCommand,
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
        handler: runUpdateCommand,
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
        handler: runCancelCommand,
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
        handler: runTruncateCommand,
      }),
      stop: command({
        name: "stop",
        description: "Stop a running workforce runtime for one repository",
        args: { root, daemonUrl },
        handler: runStopCommand,
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
