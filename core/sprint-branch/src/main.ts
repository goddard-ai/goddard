#!/usr/bin/env node
import { command, flag, option, optional, run, string, subcommands } from "cmd-ts"

import { GitCommandError, runGit } from "./git"
import { SprintInferenceError } from "./state"
import { buildStatusReport, formatDoctorReport, formatStatusReport } from "./status"
import type { SprintStatusReport } from "./types"

function sprintOption() {
  return option({
    type: optional(string),
    long: "sprint",
    description:
      "Explicit sprint name override; use only when inference is impossible or intentionally overridden",
  })
}

function jsonFlag() {
  return flag({
    long: "json",
    description: "Print machine-readable JSON output",
  })
}

function dryRunFlag() {
  return flag({
    long: "dry-run",
    description: "Print the intended operation without changing files or branches",
  })
}

/** Runs the sprint-branch CLI entrypoint against one argv payload. */
export async function main(argv: string[]) {
  const commonReadArgs = {
    sprint: sprintOption(),
    json: jsonFlag(),
  }
  const commonMutationArgs = {
    sprint: sprintOption(),
    dryRun: dryRunFlag(),
    json: jsonFlag(),
  }

  const app = subcommands({
    name: "sprint-branch",
    description: "Inspect and transition sprint review branches safely",
    cmds: {
      status: command({
        name: "status",
        description: "Inspect sprint branch state without changing anything",
        args: commonReadArgs,
        handler: async ({ sprint, json }) => {
          const { report, diagnostics } = await buildStatusReport({ cwd: process.cwd(), sprint })
          if (!report) {
            writeOutput(
              json,
              { ok: false, diagnostics },
              diagnostics
                .map((diagnostic) => `[${diagnostic.severity}] ${diagnostic.message}`)
                .join("\n"),
            )
            process.exitCode = 1
            return
          }

          writeOutput(json, report, formatStatusReport(report))
          if (!report.ok) {
            process.exitCode = 1
          }
        },
      }),
      diff: command({
        name: "diff",
        description: "Print or run the sprint review diff command",
        args: {
          ...commonReadArgs,
          runDiff: flag({
            long: "run",
            description: "Run git diff instead of only printing the command",
          }),
          nameOnly: flag({
            long: "name-only",
            description: "Use git diff --name-only",
          }),
          stat: flag({
            long: "stat",
            description: "Use git diff --stat",
          }),
        },
        handler: async ({ sprint, json, runDiff, nameOnly, stat }) => {
          const { report, diagnostics } = await buildStatusReport({ cwd: process.cwd(), sprint })
          if (!report) {
            writeOutput(json, { ok: false, diagnostics }, "Invalid sprint branch state.")
            process.exitCode = 1
            return
          }

          const validation = validateDiffState(report)
          const args = [
            "diff",
            ...(nameOnly ? ["--name-only"] : []),
            ...(stat ? ["--stat"] : []),
            `${report.state.branches.approved}...${report.state.branches.review}`,
          ]
          const commandLine = `git ${args.join(" ")}`

          if (validation.length > 0) {
            writeOutput(
              json,
              { ok: false, command: commandLine, diagnostics: validation },
              validation
                .map((diagnostic) => `[${diagnostic.severity}] ${diagnostic.message}`)
                .join("\n"),
            )
            process.exitCode = 1
            return
          }

          if (!runDiff) {
            writeOutput(json, { ok: true, command: commandLine, args }, commandLine)
            return
          }

          const output = await runGit(report.rootDir, args)
          writeOutput(json, { ok: true, command: commandLine, args, output }, output.trimEnd())
        },
      }),
      doctor: command({
        name: "doctor",
        description: "Detect and explain inconsistent sprint branch state",
        args: commonReadArgs,
        handler: async ({ sprint, json }) => {
          const { report, diagnostics } = await buildStatusReport({ cwd: process.cwd(), sprint })
          if (!report) {
            writeOutput(
              json,
              { ok: false, diagnostics },
              diagnostics
                .map((diagnostic) => `[${diagnostic.severity}] ${diagnostic.message}`)
                .join("\n"),
            )
            process.exitCode = 1
            return
          }

          writeOutput(
            json,
            {
              ok: report.ok,
              diagnostics: report.diagnostics,
              nextSafeCommand: report.blocked.nextSafeCommand,
              status: report,
            },
            formatDoctorReport(report),
          )
          if (!report.ok) {
            process.exitCode = 1
          }
        },
      }),
      init: command({
        name: "init",
        description: "Create the sprint branch scaffold",
        args: {
          ...commonMutationArgs,
          base: option({
            type: string,
            long: "base",
            defaultValue: () => "main",
            description: "Base branch for the sprint approved branch",
          }),
        },
        handler: async (args) => {
          writeNotImplemented("init", args)
        },
      }),
      start: command({
        name: "start",
        description: "Begin the next planned task on the valid rolling branch",
        args: {
          ...commonMutationArgs,
          task: option({
            type: string,
            long: "task",
            description: "Task file stem, such as 020-task-name",
          }),
        },
        handler: async (args) => {
          writeNotImplemented("start", args)
        },
      }),
      feedback: command({
        name: "feedback",
        description: "Prepare for human feedback on the review branch",
        args: commonMutationArgs,
        handler: async (args) => {
          writeNotImplemented("feedback", args)
        },
      }),
      resume: command({
        name: "resume",
        description: "Return to interrupted or dependent work after feedback changes",
        args: commonMutationArgs,
        handler: async (args) => {
          writeNotImplemented("resume", args)
        },
      }),
      approve: command({
        name: "approve",
        description: "Promote reviewed work into the approved branch",
        args: commonMutationArgs,
        handler: async (args) => {
          writeNotImplemented("approve", args)
        },
      }),
      finalize: command({
        name: "finalize",
        description: "Prepare the completed review branch for the human's final merge",
        args: {
          ...commonMutationArgs,
          base: option({
            type: optional(string),
            long: "base",
            description: "Explicit base branch override for recovery",
          }),
        },
        handler: async (args) => {
          writeNotImplemented("finalize", args)
        },
      }),
    },
  })

  await run(app, argv)
}

function validateDiffState(report: SprintStatusReport) {
  const diagnostics = []

  if (!report.branches.approved.exists) {
    diagnostics.push({
      severity: "error" as const,
      code: "approved_branch_missing",
      message: `Approved branch ${report.state.branches.approved} does not exist.`,
    })
  }
  if (!report.branches.review.exists) {
    diagnostics.push({
      severity: "error" as const,
      code: "review_branch_missing",
      message: `Review branch ${report.state.branches.review} does not exist.`,
    })
  }
  if (report.ancestry.reviewDescendsFromApproved === false) {
    diagnostics.push({
      severity: "error" as const,
      code: "review_not_based_on_approved",
      message: `${report.state.branches.review} does not descend from ${report.state.branches.approved}.`,
    })
  }

  return diagnostics
}

function writeNotImplemented(commandName: string, args: { json: boolean; dryRun: boolean }) {
  const payload = {
    ok: false,
    command: commandName,
    implemented: false,
    dryRun: args.dryRun,
    message: `${commandName} is reserved but not implemented in Phase 1.`,
  }
  writeOutput(args.json, payload, payload.message)
  process.exitCode = 1
}

function writeOutput(json: boolean, value: unknown, text: string) {
  if (json) {
    console.log(JSON.stringify(value, null, 2))
    return
  }

  if (text.length > 0) {
    console.log(text)
  }
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    if (error instanceof SprintInferenceError) {
      console.error(error.message)
      for (const diagnostic of error.diagnostics) {
        console.error(`[${diagnostic.severity}] ${diagnostic.message}`)
        if (diagnostic.suggestion) {
          console.error(`suggestion: ${diagnostic.suggestion}`)
        }
      }
      process.exit(1)
    }

    if (error instanceof GitCommandError) {
      console.error(`git ${error.args.join(" ")} failed`)
      console.error(error.stderr || error.message)
      process.exit(1)
    }

    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  })
}
