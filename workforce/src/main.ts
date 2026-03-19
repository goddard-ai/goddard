#!/usr/bin/env node
import { cancel, intro, isCancel, multiselect, outro } from "@clack/prompts"
import { command, option, optional, runSafely, string, subcommands } from "cmd-ts"
import {
  cancelWorkforceRequest,
  createWorkforceRequest,
  discoverWorkforceInitCandidates,
  getWorkforce,
  initializeWorkforce,
  listWorkforces,
  resolveRepositoryRoot,
  startWorkforce,
  stopWorkforce,
  truncateWorkforce,
  updateWorkforceRequest,
  type DiscoveredWorkforcePackage,
} from "@goddard-ai/sdk/node"

function formatPackageLabel(pkg: DiscoveredWorkforcePackage): string {
  return pkg.relativeDir === "."
    ? `${pkg.name} (repository root)`
    : `${pkg.name} (${pkg.relativeDir})`
}

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

export async function runStartCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const workforce = await startWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(`Started workforce for ${workforce.rootDir}.`)
}

export async function runStatusCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const workforce = await getWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(JSON.stringify(workforce, null, 2))
}

export async function runListCommand(args: { daemonUrl?: string }): Promise<void> {
  const workforces = await listWorkforces({
    daemonUrl: args.daemonUrl,
  })
  console.log(JSON.stringify(workforces, null, 2))
}

export async function runRequestCommand(args: {
  root: string
  targetAgentId: string
  message: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  const response = await createWorkforceRequest(
    {
      rootDir: repositoryRoot,
      targetAgentId: args.targetAgentId,
      message: args.message,
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Queued workforce request ${response.requestId ?? "unknown"}.`)
}

export async function runCreateCommand(args: {
  root: string
  message: string
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
      message: args.message,
      intent: "create",
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Queued create request ${response.requestId ?? "unknown"}.`)
}

export async function runUpdateCommand(args: {
  root: string
  requestId: string
  message: string
  daemonUrl?: string
}): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await updateWorkforceRequest(
    {
      rootDir: repositoryRoot,
      requestId: args.requestId,
      message: args.message,
    },
    {
      daemonUrl: args.daemonUrl,
    },
  )
  console.log(`Updated workforce request ${args.requestId}.`)
}

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

export async function runStopCommand(args: { root: string; daemonUrl?: string }): Promise<void> {
  const repositoryRoot = await resolveRepositoryRoot(args.root)
  await stopWorkforce(repositoryRoot, {
    daemonUrl: args.daemonUrl,
  })
  console.log(`Stopped workforce for ${repositoryRoot}.`)
}

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
            type: string,
            long: "message",
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
            type: string,
            long: "message",
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
            type: string,
            long: "message",
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
