#!/usr/bin/env bun
import { resolve } from "node:path"
import { parseArgs } from "node:util"

import { creativeWeaverPipelineId } from "./pipeline.ts"
import { createWeaverPayload, type Emotion } from "./weaver.ts"

const emotions = ["grief", "dread", "awe", "tension", "calm", "obsession"] satisfies Emotion[]
const modes = ["payload", "spawn", "inspect"] as const

type Mode = (typeof modes)[number]

function usage() {
  return `Usage:
  creative-weaver-poc --premise "A scene premise" [options]
  creative-weaver-poc --mode spawn --premise "A scene premise" [options]
  creative-weaver-poc --mode inspect --run-id plr_...

Options:
  --mode <name>       payload, spawn, or inspect. Defaults to payload.
  --premise <text>     One-sentence scene premise.
  --emotion <name>     Scene emotion: ${emotions.join(", ")}. Defaults to tension.
  --seed <number>      Deterministic seed. Defaults to 1.
  --words <number>     Target Artisan word count. Defaults to 500.
  --cwd <path>         Project root containing .goddard/pipelines. Defaults to cwd.
  --run-id <id>        Pipeline run id for inspect mode.
  --advance            After spawn, advance the run until it waits, fails, or completes.
  --daemon-url <url>   Optional daemon URL override.
  --help               Show this help text.
`
}

function readCliOptions() {
  const parsed = parseArgs({
    args: process.argv.slice(2),
    options: {
      mode: { type: "string" },
      premise: { type: "string" },
      emotion: { type: "string" },
      seed: { type: "string" },
      words: { type: "string" },
      cwd: { type: "string" },
      "run-id": { type: "string" },
      advance: { type: "boolean" },
      "daemon-url": { type: "string" },
      help: { type: "boolean" },
    },
  })

  if (parsed.values.help) {
    process.stdout.write(usage())
    process.exit(0)
  }

  const mode = parseMode(parsed.values.mode ?? "payload")

  if (mode === "inspect") {
    const runId = parsed.values["run-id"]
    if (!runId) {
      throw new Error("Missing required --run-id option for inspect mode.")
    }

    return {
      mode,
      cwd: resolve(parsed.values.cwd ?? process.cwd()),
      daemonUrl: parsed.values["daemon-url"],
      runId,
    }
  }

  const premise = parsed.values.premise
  if (!premise) {
    throw new Error("Missing required --premise option.")
  }

  const emotion = parseEmotion(parsed.values.emotion ?? "tension")

  return {
    mode,
    premise,
    emotion,
    seed: parseInteger("--seed", parsed.values.seed ?? "1"),
    targetWords: parseInteger("--words", parsed.values.words ?? "500"),
    cwd: resolve(parsed.values.cwd ?? process.cwd()),
    daemonUrl: parsed.values["daemon-url"],
    advance: parsed.values.advance ?? false,
  }
}

function parseMode(value: string) {
  if (!modes.includes(value as Mode)) {
    throw new Error(`Invalid --mode value "${value}". Use one of: ${modes.join(", ")}.`)
  }

  return value as Mode
}

function parseEmotion(value: string) {
  if (!emotions.includes(value as Emotion)) {
    throw new Error(`Invalid --emotion value "${value}". Use one of: ${emotions.join(", ")}.`)
  }

  return value as Emotion
}

function parseInteger(name: string, value: string) {
  const parsed = Number.parseInt(value, 10)
  if (!Number.isInteger(parsed) || String(parsed) !== value) {
    throw new Error(`Invalid ${name} value "${value}". Use a whole number.`)
  }

  return parsed
}

try {
  const options = readCliOptions()
  const result =
    options.mode === "payload" ? createWeaverPayload(options) : await runPipelineCommand(options)

  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`)
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`${message}\n\n${usage()}`)
  process.exit(1)
}

async function runPipelineCommand(options: ReturnType<typeof readCliOptions>) {
  const { GoddardSdk } = await import("../../../core/sdk/src/node/index.ts")
  const sdk = new GoddardSdk(
    options.daemonUrl
      ? {
          daemonUrl: options.daemonUrl,
        }
      : {},
  )

  if (options.mode === "inspect") {
    return sdk.pipeline.getRun({ id: options.runId as `plr_${string}` })
  }

  const spawned = await sdk.pipeline.spawnRun({
    cwd: options.cwd,
    pipelineId: creativeWeaverPipelineId,
    inputs: {
      premise: options.premise,
      emotion: options.emotion,
      seed: options.seed,
      targetWords: options.targetWords,
    },
    origin: "cli",
    visibility: "visible",
  })

  if (!options.advance) {
    return spawned
  }

  return sdk.pipeline.advanceRun({ id: spawned.run.id })
}
