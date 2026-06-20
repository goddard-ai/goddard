#!/usr/bin/env bun
import { getGoddardLogDatabasePath } from "@goddard-ai/paths/node"
import {
  command,
  extendType,
  flag,
  multioption,
  option,
  optional,
  positional,
  run,
  string,
  subcommands,
  type Type,
} from "cmd-ts"

import {
  createLogStore,
  formatLogEntry,
  type DebugScopeSummary,
  type LogEntry,
  type LogLevel,
  type LogQuery,
} from "./index.ts"

type CliOptions = LogQuery & {
  json?: boolean
  properties: Record<string, string>
}

type ScopeListOptions = {
  json?: boolean
  prefix?: string
  scope?: string
  since?: string
}

const defaultTailIntervalMs = 1000
const defaultMinimumLevel = "info" satisfies LogLevel
const positiveInteger = extendType(string, {
  displayName: "positive-integer",
  async from(value) {
    return parsePositiveInteger(value)
  },
})
const sinceDate = extendType(string, {
  displayName: "date-or-duration",
  async from(value) {
    return parseSince(value)
  },
})
const logLevel = extendType(string, {
  displayName: "debug|info|log|warn|error",
  async from(value) {
    if (
      value !== "debug" &&
      value !== "info" &&
      value !== "log" &&
      value !== "warn" &&
      value !== "error"
    ) {
      throw new Error("--level must be one of debug, info, log, warn, or error")
    }

    return value
  },
})
const propertyFilters: Type<string[], Record<string, string>> = {
  displayName: "path=value",
  async from(values) {
    const properties: Record<string, string> = {}
    for (const value of values) {
      const [key, propertyValue] = splitPropertyFilter(value)
      properties[key] = propertyValue
    }
    return properties
  },
}

const jsonFlag = flag({
  long: "json",
  description: "Write output as JSON.",
})

const queryArgs = {
  json: jsonFlag,
  afterId: option({
    type: optional(positiveInteger),
    long: "after-id",
    description: "Return entries after this log id.",
  }),
  beforeId: option({
    type: optional(positiveInteger),
    long: "before-id",
    description: "Return entries before this log id.",
  }),
  limit: option({
    type: optional(positiveInteger),
    long: "limit",
    description: "Maximum number of entries to return.",
  }),
  level: option({
    type: optional(logLevel),
    long: "level",
    description: "Minimum log level to show. Defaults to info.",
  }),
  debugScope: option({
    type: optional(string),
    long: "debug",
    description: "Show debug entries for this debug scope prefix.",
  }),
  since: option({
    type: optional(sinceDate),
    long: "since",
    description: "Return entries after an ISO date or relative duration such as 30m, 2h, or 1d.",
  }),
  scope: option({
    type: optional(string),
    long: "scope",
    description: "Return entries for one log scope.",
  }),
  grep: option({
    type: optional(string),
    long: "grep",
    description: "Return entries whose message or properties include this text.",
  }),
  regex: option({
    type: optional(string),
    long: "regex",
    description: "Return entries whose message or properties match this regex.",
  }),
  properties: multioption({
    type: propertyFilters,
    long: "property",
    description:
      "Return entries with a matching property path such as method=name or ipcRequest.opId=id.",
    defaultValue: () => ({}),
  }),
}

const app = subcommands({
  name: "goddard:logs",
  description: "Inspect Goddard logs.",
  cmds: {
    page: command({
      name: "page",
      description: "Page Goddard logs.",
      args: queryArgs,
      handler: page,
    }),
    path: command({
      name: "path",
      description: "Print the canonical log database path.",
      args: {},
      handler: () => {
        console.log(getGoddardLogDatabasePath())
      },
    }),
    scopes: command({
      name: "scopes",
      description: "List observed debug scopes, grouped by log scope.",
      args: {
        json: flag({
          long: "json",
          description: "Write debug scope summaries as JSON.",
        }),
        since: option({
          type: optional(sinceDate),
          long: "since",
          description: "Return scopes observed after an ISO date or duration such as 30m.",
        }),
        scope: option({
          type: optional(string),
          long: "scope",
          description: "Return debug scopes for one log scope.",
        }),
        prefix: option({
          type: optional(string),
          long: "prefix",
          description: "Return debug scopes with this debug scope prefix.",
        }),
      },
      handler: listScopes,
    }),
    expand: command({
      name: "expand",
      description: "Expand a collapsed log value.",
      args: {
        id: positional({
          type: string,
          displayName: "collapsed_id",
          description: "Collapsed log value id.",
        }),
        json: jsonFlag,
      },
      handler: ({ id, ...options }) => expand(id, options),
    }),
    tail: command({
      name: "tail",
      description: "Continuously print new matching log entries.",
      args: queryArgs,
      handler: tail,
    }),
  },
})

export async function main(argv = process.argv.slice(2)) {
  await run(app, argv)
}

function page(options: CliOptions) {
  const store = createLogStore()
  try {
    const query = normalizeQueryOptions(options)
    const entries = store.query(query)
    writeEntries(entries, query)
  } finally {
    store.close()
  }
}

function listScopes(options: ScopeListOptions) {
  const store = createLogStore()
  try {
    const summaries = store.listDebugScopes(options)
    writeScopeSummaries(summaries, options)
  } finally {
    store.close()
  }
}

async function tail(options: CliOptions) {
  let afterId = options.afterId

  while (true) {
    const store = createLogStore()
    try {
      const entries = store.query({
        ...normalizeQueryOptions(options),
        afterId,
        beforeId: undefined,
      })
      if (entries.length > 0) {
        writeEntries(entries, { ...options, afterId, beforeId: undefined })
        afterId = entries.at(-1)?.id
      }
    } finally {
      store.close()
    }

    await new Promise((resolve) => setTimeout(resolve, defaultTailIntervalMs))
  }
}

function writeScopeSummaries(summaries: DebugScopeSummary[], options: ScopeListOptions) {
  if (options.json) {
    console.log(
      JSON.stringify(
        {
          items: summaries,
        },
        null,
        2,
      ),
    )
    return
  }

  if (summaries.length === 0) {
    console.log("No debug scopes observed.")
    return
  }

  const groups = Map.groupBy(summaries, (summary) => summary.scope)
  let firstGroup = true
  for (const [scope, group] of groups) {
    if (!firstGroup) {
      console.log("")
    }
    firstGroup = false
    console.log(scope)
    console.log(formatScopeSummaryHeader(group))
    for (const summary of group) {
      console.log(formatScopeSummary(summary, group))
    }
  }
}

function formatScopeSummaryHeader(summaries: DebugScopeSummary[]) {
  return [
    "debugScope".padEnd(readMaxDebugScopeLength(summaries)),
    "count".padStart(readMaxCountLength(summaries)),
    "firstSeen".padEnd(24),
    "lastSeen",
  ].join("  ")
}

function formatScopeSummary(summary: DebugScopeSummary, summaries: DebugScopeSummary[]) {
  return [
    summary.debugScope.padEnd(readMaxDebugScopeLength(summaries)),
    String(summary.count).padStart(readMaxCountLength(summaries)),
    summary.firstSeen.padEnd(24),
    summary.lastSeen,
  ].join("  ")
}

function readMaxDebugScopeLength(summaries: DebugScopeSummary[]) {
  return Math.max("debugScope".length, ...summaries.map((summary) => summary.debugScope.length))
}

function readMaxCountLength(summaries: DebugScopeSummary[]) {
  return Math.max("count".length, ...summaries.map((summary) => String(summary.count).length))
}

function expand(id: string, options: { json?: boolean }) {
  const store = createLogStore()
  try {
    const value = store.expand(id)
    if (!value) {
      throw new Error(`No collapsed log value found for ${id}`)
    }

    if (options.json) {
      console.log(JSON.stringify(value, null, 2))
      return
    }

    console.log(JSON.stringify(value.body, null, 2))
  } finally {
    store.close()
  }
}

function normalizeQueryOptions(options: CliOptions): CliOptions {
  return {
    ...options,
    level: options.debugScope ? "debug" : (options.level ?? defaultMinimumLevel),
  }
}

function writeEntries(entries: LogEntry[], options: CliOptions) {
  if (options.json) {
    console.log(
      JSON.stringify(
        {
          items: entries,
          page: createPageCursors(entries),
        },
        null,
        2,
      ),
    )
    return
  }

  if (entries.length === 0) {
    console.log("No logs matched.")
    return
  }

  for (const entry of entries) {
    console.log(formatLogEntry(entry))
  }
}

function createPageCursors(entries: LogEntry[]) {
  const firstId = entries[0]?.id ?? null
  const lastId = entries.at(-1)?.id ?? null

  return {
    firstId,
    lastId,
    next: lastId == null ? null : { afterId: lastId },
    prev: firstId == null ? null : { beforeId: firstId },
  }
}

function parsePositiveInteger(value: string) {
  const parsed = Number(value)
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error("Value must be a positive integer")
  }

  return parsed
}

function parseSince(value: string) {
  const relative = value.match(/^(\d+)(m|h|d)$/)
  if (relative) {
    const amount = Number(relative[1])
    const unit = relative[2]
    const multiplier = unit === "m" ? 60_000 : unit === "h" ? 3_600_000 : 86_400_000
    return new Date(Date.now() - amount * multiplier).toISOString()
  }

  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    throw new Error("--since must be an ISO date or relative duration such as 30m, 2h, or 1d")
  }

  return date.toISOString()
}

function splitPropertyFilter(value: string) {
  const separatorIndex = value.indexOf("=")
  if (separatorIndex <= 0) {
    throw new Error("--property must use key=value syntax")
  }

  return [value.slice(0, separatorIndex), value.slice(separatorIndex + 1)] as const
}

if (import.meta.main) {
  await main()
}
