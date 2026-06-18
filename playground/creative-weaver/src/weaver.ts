import { clamp, sort } from "radashi"

export type Emotion = "grief" | "dread" | "awe" | "tension" | "calm" | "obsession"

export type WeaverInput = {
  premise: string
  emotion: Emotion
  seed: number
  targetWords: number
}

type ToneVector = {
  cold: number
  motion: number
  intimacy: number
  decay: number
  scale: number
}

type AnchorSnippet = {
  id: string
  emotions: Emotion[]
  vector: ToneVector
  text: string
}

const emotionVectors = {
  grief: { cold: 0.7, motion: 0.2, intimacy: 0.9, decay: 0.8, scale: 0.4 },
  dread: { cold: 0.8, motion: 0.5, intimacy: 0.3, decay: 0.7, scale: 0.7 },
  awe: { cold: 0.4, motion: 0.4, intimacy: 0.2, decay: 0.1, scale: 1 },
  tension: { cold: 0.5, motion: 0.8, intimacy: 0.6, decay: 0.4, scale: 0.5 },
  calm: { cold: 0.3, motion: 0.2, intimacy: 0.7, decay: 0.2, scale: 0.3 },
  obsession: { cold: 0.6, motion: 0.6, intimacy: 0.8, decay: 0.5, scale: 0.4 },
} satisfies Record<Emotion, ToneVector>

const anchors: AnchorSnippet[] = [
  {
    id: "dying-star",
    emotions: ["grief", "awe"],
    vector: { cold: 0.9, motion: 0.3, intimacy: 0.1, decay: 0.8, scale: 1 },
    text: "The last light of a dying star keeps traveling after the star has already collapsed.",
  },
  {
    id: "ship-bell",
    emotions: ["grief", "dread", "tension"],
    vector: { cold: 0.8, motion: 0.4, intimacy: 0.5, decay: 0.6, scale: 0.5 },
    text: "A ship bell can sound clean while the hull beneath it is taking on water.",
  },
  {
    id: "clock-spring",
    emotions: ["tension", "obsession"],
    vector: { cold: 0.4, motion: 0.9, intimacy: 0.4, decay: 0.4, scale: 0.2 },
    text: "A wound spring spends every second trying to become less itself.",
  },
  {
    id: "museum-dust",
    emotions: ["calm", "grief", "obsession"],
    vector: { cold: 0.5, motion: 0.1, intimacy: 0.8, decay: 0.7, scale: 0.3 },
    text: "Museum dust gathers most thickly where careful hands have stopped touching things.",
  },
  {
    id: "deep-current",
    emotions: ["dread", "awe", "calm"],
    vector: { cold: 0.7, motion: 0.6, intimacy: 0.2, decay: 0.2, scale: 0.9 },
    text: "A deep current can move an entire coast without wrinkling the surface.",
  },
  {
    id: "left-hand",
    emotions: ["tension", "obsession"],
    vector: { cold: 0.4, motion: 0.5, intimacy: 0.9, decay: 0.3, scale: 0.1 },
    text: "A person's left hand may betray the conversation their mouth is trying to win.",
  },
  {
    id: "frosted-orchard",
    emotions: ["calm", "grief", "awe"],
    vector: { cold: 0.8, motion: 0.1, intimacy: 0.5, decay: 0.3, scale: 0.4 },
    text: "An orchard under frost looks preserved until the first branch snaps.",
  },
  {
    id: "archive-ink",
    emotions: ["obsession", "calm", "grief"],
    vector: { cold: 0.5, motion: 0.1, intimacy: 0.9, decay: 0.5, scale: 0.2 },
    text: "Old ink often darkens at the edge of a word, as if meaning pools there.",
  },
]

const strategies = [
  "Let one practical object carry the emotional argument of the scene.",
  "Make the point-of-view character notice a small bodily detail before every direct answer.",
  "Describe light by what it refuses to reveal.",
  "Use an extended mechanical metaphor, but never name the machine outright.",
  "Let the setting behave like it remembers an earlier version of the relationship.",
  "Avoid naming the central emotion; show its pressure through misread gestures.",
]

export function createWeaverPayload(input: WeaverInput) {
  const sceneVector = emotionVectors[input.emotion]
  const random = createSeededRandom(input.seed)
  const strategy = pick(strategies, random)
  const selectedAnchors = sampleAnchors({
    emotion: input.emotion,
    sceneVector,
    random,
  })
  const entropy = resolveEntropy(input.emotion)
  const ledger = createArchitectLedger(input)

  return {
    ledger,
    chaos: {
      seed: input.seed,
      strategy,
      anchors: selectedAnchors.map((anchor) => ({
        id: anchor.id,
        text: anchor.text,
      })),
      entropy,
    },
    artisanPrompt: createArtisanPrompt({
      ledger,
      strategy,
      anchors: selectedAnchors,
      entropy,
      targetWords: input.targetWords,
    }),
  }
}

function createArchitectLedger(input: WeaverInput) {
  return {
    premise: input.premise,
    narrativeGoal: "Advance the scene through one irreversible emotional turn.",
    emotionalState: input.emotion,
    pacing: input.emotion === "tension" || input.emotion === "dread" ? "tightening" : "measured",
    sensoryFocus: resolveSensoryFocus(input.emotion),
    continuityRules: [
      "Do not resolve the whole story.",
      "Keep character motivation legible even when the imagery becomes strange.",
      "Every metaphor must alter how the reader understands the action.",
    ],
  }
}

function resolveSensoryFocus(emotion: Emotion) {
  switch (emotion) {
    case "grief":
      return "temperature, silence, and absent touch"
    case "dread":
      return "pressure, distant sound, and blocked exits"
    case "awe":
      return "scale, reflected light, and breath"
    case "tension":
      return "hands, timing, and interrupted motion"
    case "calm":
      return "texture, steady rhythm, and softened edges"
    case "obsession":
      return "repeated details, counting, and fixation"
  }
}

function resolveEntropy(emotion: Emotion) {
  switch (emotion) {
    case "dread":
    case "tension":
      return { temperature: 1.15, topP: 0.86 }
    case "awe":
    case "obsession":
      return { temperature: 1.0, topP: 0.9 }
    case "grief":
      return { temperature: 0.92, topP: 0.88 }
    case "calm":
      return { temperature: 0.72, topP: 0.94 }
  }
}

function sampleAnchors(input: { emotion: Emotion; sceneVector: ToneVector; random: () => number }) {
  const candidates = anchors.filter((anchor) => anchor.emotions.includes(input.emotion))
  const ranked = sort(
    candidates.map((anchor) => ({
      anchor,
      distance: vectorDistance(input.sceneVector, anchor.vector),
    })),
    (candidate) => candidate.distance,
  )
  const middleDistance = ranked.slice(1, Math.max(3, ranked.length - 1))
  const pool = middleDistance.length >= 2 ? middleDistance : ranked
  const first = pick(pool, input.random).anchor
  const remaining = pool.filter((candidate) => candidate.anchor.id !== first.id)
  const second = pick(remaining.length > 0 ? remaining : ranked, input.random).anchor

  return [first, second]
}

function vectorDistance(left: ToneVector, right: ToneVector) {
  const keys = Object.keys(left) as Array<keyof ToneVector>
  const total = keys.reduce((distance, key) => distance + (left[key] - right[key]) ** 2, 0)

  return Math.sqrt(total)
}

function createArtisanPrompt(input: {
  ledger: ReturnType<typeof createArchitectLedger>
  strategy: string
  anchors: AnchorSnippet[]
  entropy: { temperature: number; topP: number }
  targetWords: number
}) {
  const anchorText = input.anchors.map((anchor) => `- ${anchor.text}`).join("\n")

  return `Write approximately ${input.targetWords} words of literary prose.

Structure:
- Premise: ${input.ledger.premise}
- Narrative goal: ${input.ledger.narrativeGoal}
- Emotional state: ${input.ledger.emotionalState}
- Pacing: ${input.ledger.pacing}
- Sensory focus: ${input.ledger.sensoryFocus}

Intentional chaos:
- Oblique strategy: ${input.strategy}
- Sensory anchors:
${anchorText}

Continuity rules:
${input.ledger.continuityRules.map((rule) => `- ${rule}`).join("\n")}

Suggested model settings:
- temperature: ${input.entropy.temperature}
- top_p: ${input.entropy.topP}

Blend the anchors into the scene as metaphor, attention, or physical detail. Do not quote them as exposition.`
}

function pick<T>(values: T[], random: () => number) {
  const index = Math.floor(random() * values.length)

  return values[clamp(index, 0, values.length - 1)] as T
}

function createSeededRandom(seed: number) {
  let state = seed >>> 0

  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let value = state
    value = Math.imul(value ^ (value >>> 15), value | 1)
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61)

    return ((value ^ (value >>> 14)) >>> 0) / 4294967296
  }
}
