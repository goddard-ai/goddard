#!/usr/bin/env bun
import { getGoddardLogDatabasePath } from "@goddard-ai/paths/node"

import { createLogStore, formatLogEntry, type LogEntry, type LogQuery } from "./index.ts"

type CliOptions = LogQuery & {
  json?: boolean
  properties: Record<string, string>
}

const defaultTailIntervalMs = 1000

export async function main(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv

  if (command === "path") {
    console.log(getGoddardLogDatabasePath())
    return
  }

  if (command === "expand") {
    expand(rest)
    return
  }

  if (command === "tail") {
    await tail(parseOptions(rest))
    return
  }

  page(parseOptions(argv))
}

function page(options: CliOptions) {
  const store = createLogStore()
  try {
    const entries = store.query(options)
    writeEntries(entries, options)
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
        ...options,
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

function expand(argv: string[]) {
  const options = parseOptions(argv)
  const id = argv.find((argument) => !argument.startsWith("--"))

  if (!id) {
    throw new Error("Usage: pnpm goddard:logs expand <collapsed_id>")
  }

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

  const cursors = createPageCursors(entries)
  if (entries.length === 0) {
    console.log("No logs matched.")
    return
  }

  console.log(`firstId: ${cursors.firstId}`)
  console.log(`lastId: ${cursors.lastId}`)
  console.log(`prev: pnpm goddard:logs --before-id ${cursors.firstId}`)
  console.log(`next: pnpm goddard:logs --after-id ${cursors.lastId}`)
  console.log("")

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

function parseOptions(argv: string[]): CliOptions {
  const options: CliOptions = {
    properties: {},
  }

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index]

    if (!argument?.startsWith("--")) {
      continue
    }

    if (argument === "--json") {
      options.json = true
      continue
    }

    const value = argv[index + 1]
    if (!value) {
      throw new Error(`${argument} requires a value`)
    }
    index += 1

    if (argument === "--after-id") {
      options.afterId = parsePositiveInteger(value, argument)
    } else if (argument === "--before-id") {
      options.beforeId = parsePositiveInteger(value, argument)
    } else if (argument === "--limit") {
      options.limit = parsePositiveInteger(value, argument)
    } else if (argument === "--since") {
      options.since = parseSince(value)
    } else if (argument === "--scope") {
      options.scope = value
    } else if (argument === "--grep") {
      options.grep = value
    } else if (argument === "--regex") {
      options.regex = value
    } else if (argument === "--property") {
      const [key, propertyValue] = splitPropertyFilter(value)
      options.properties[key] = propertyValue
    } else {
      throw new Error(`Unknown option ${argument}`)
    }
  }

  return options
}

function parsePositiveInteger(value: string, option: string) {
  const parsed = Number(value)
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error(`${option} must be a positive integer`)
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
