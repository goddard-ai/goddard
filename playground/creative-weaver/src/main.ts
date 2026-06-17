#!/usr/bin/env bun
import { parseArgs } from "node:util"

import { createWeaverPayload, type Emotion } from "./weaver.ts"

const emotions = ["grief", "dread", "awe", "tension", "calm", "obsession"] satisfies Emotion[]

function usage() {
  return `Usage:
  creative-weaver-poc --premise "A scene premise" [options]

Options:
  --premise <text>     One-sentence scene premise.
  --emotion <name>     Scene emotion: ${emotions.join(", ")}. Defaults to tension.
  --seed <number>      Deterministic seed. Defaults to 1.
  --words <number>     Target Artisan word count. Defaults to 500.
  --help               Show this help text.
`
}

function readCliOptions() {
  const parsed = parseArgs({
    args: process.argv.slice(2),
    options: {
      premise: { type: "string" },
      emotion: { type: "string" },
      seed: { type: "string" },
      words: { type: "string" },
      help: { type: "boolean" },
    },
  })

  if (parsed.values.help) {
    process.stdout.write(usage())
    process.exit(0)
  }

  const premise = parsed.values.premise
  if (!premise) {
    throw new Error("Missing required --premise option.")
  }

  const emotion = parseEmotion(parsed.values.emotion ?? "tension")

  return {
    premise,
    emotion,
    seed: parseInteger("--seed", parsed.values.seed ?? "1"),
    targetWords: parseInteger("--words", parsed.values.words ?? "500"),
  }
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
  const payload = createWeaverPayload(readCliOptions())
  process.stdout.write(`${JSON.stringify(payload, null, 2)}\n`)
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  process.stderr.write(`${message}\n\n${usage()}`)
  process.exit(1)
}
