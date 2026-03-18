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
  })
  const root = option({
    type: string,
    long: "root",
    defaultValue: () => process.cwd(),
  })

  const app = subcommands({
    name: "goddard-workforce",
    cmds: {
      init: command({
        name: "init",
        args: { root },
        handler: runInitCommand,
      }),
      start: command({
        name: "start",
        args: { root, daemonUrl },
        handler: runStartCommand,
      }),
      status: command({
        name: "status",
        args: { root, daemonUrl },
        handler: runStatusCommand,
      }),
      list: command({
        name: "list",
        args: { daemonUrl },
        handler: runListCommand,
      }),
      request: command({
        name: "request",
        args: {
          root,
          daemonUrl,
          targetAgentId: option({
            type: string,
            long: "target-agent-id",
          }),
          message: option({
            type: string,
            long: "message",
          }),
        },
        handler: runRequestCommand,
      }),
      update: command({
        name: "update",
        args: {
          root,
          daemonUrl,
          requestId: option({
            type: string,
            long: "request-id",
          }),
          message: option({
            type: string,
            long: "message",
          }),
        },
        handler: runUpdateCommand,
      }),
      cancel: command({
        name: "cancel",
        args: {
          root,
          daemonUrl,
          requestId: option({
            type: string,
            long: "request-id",
          }),
          reason: option({
            type: optional(string),
            long: "reason",
          }),
        },
        handler: runCancelCommand,
      }),
      truncate: command({
        name: "truncate",
        args: {
          root,
          daemonUrl,
          agentId: option({
            type: optional(string),
            long: "agent-id",
          }),
          reason: option({
            type: optional(string),
            long: "reason",
          }),
        },
        handler: runTruncateCommand,
      }),
      stop: command({
        name: "stop",
        args: { root, daemonUrl },
        handler: runStopCommand,
      }),
    },
  })

  const result = await runSafely(app, argv)
  if (result._tag === "error") {
    const helpResult = await runSafely(app, ["--help"])
    if (helpResult._tag === "error" && helpResult.error.config) {
      console.log(helpResult.error.config.message)
    }
    process.exit(1)
  }
}

if (import.meta.main) {
  await main(process.argv.slice(2))
}
