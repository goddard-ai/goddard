import { expect, test } from "bun:test"

import { PipelineDefinition } from "../src/schema.ts"

const validPipeline = {
  id: "creative-weaver",
  version: "0.1.0",
  name: "Creative Weaver",
  inputs: {
    premise: { type: "string" },
  },
  steps: [
    {
      id: "architect",
      kind: "agent",
      name: "Architect",
      systemPromptFile: "./prompts/architect.md",
      input: {
        premise: "$.inputs.premise",
      },
    },
    {
      id: "chaos",
      kind: "script",
      name: "Chaos Weaver",
      transformer: "creative-weaver.sample-chaos",
      input: {
        ledger: "$.steps.architect.output",
      },
    },
    {
      id: "approval",
      kind: "approval",
      name: "Review Payload",
      input: {
        payload: "$.steps.chaos.output",
      },
    },
  ],
}

test("PipelineDefinition accepts a valid linear definition", () => {
  expect(PipelineDefinition.parse(validPipeline)).toMatchObject({
    id: "creative-weaver",
    steps: [
      { id: "architect", kind: "agent" },
      { id: "chaos", kind: "script" },
      { id: "approval", kind: "approval" },
    ],
  })
})

test("PipelineDefinition rejects unknown step kinds", () => {
  expect(() =>
    PipelineDefinition.parse({
      ...validPipeline,
      steps: [
        {
          id: "mystery",
          kind: "mystery",
          name: "Mystery",
        },
      ],
    }),
  ).toThrow()
})

test("PipelineDefinition rejects missing required definition fields", () => {
  const result = PipelineDefinition.safeParse({
    id: "missing-fields",
    version: "0.1.0",
    steps: validPipeline.steps,
  })

  expect(result.success).toBe(false)
  expect(result.error?.issues).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ path: ["name"] }),
      expect.objectContaining({ path: ["inputs"] }),
    ]),
  )
})

test("PipelineDefinition rejects duplicate step ids", () => {
  const result = PipelineDefinition.safeParse({
    ...validPipeline,
    steps: [
      validPipeline.steps[0],
      {
        ...validPipeline.steps[1],
        id: "architect",
      },
    ],
  })

  expect(result.success).toBe(false)
  expect(result.error?.issues).toContainEqual(
    expect.objectContaining({
      message: 'Duplicate step id "architect".',
      path: ["steps", 1, "id"],
    }),
  )
})

test("PipelineDefinition rejects forward step references", () => {
  const result = PipelineDefinition.safeParse({
    ...validPipeline,
    steps: [
      {
        id: "architect",
        kind: "agent",
        name: "Architect",
        systemPromptFile: "./prompts/architect.md",
        input: {
          payload: "$.steps.chaos.output",
        },
      },
      validPipeline.steps[1],
    ],
  })

  expect(result.success).toBe(false)
  expect(result.error?.issues).toContainEqual(
    expect.objectContaining({
      message: 'Step input "payload" may only reference earlier step outputs.',
      path: ["steps", 0, "input", "payload"],
    }),
  )
})

test("PipelineDefinition rejects non-output step references", () => {
  const result = PipelineDefinition.safeParse({
    ...validPipeline,
    steps: [
      {
        id: "architect",
        kind: "agent",
        name: "Architect",
        systemPromptFile: "./prompts/architect.md",
      },
      {
        id: "chaos",
        kind: "script",
        name: "Chaos Weaver",
        transformer: "creative-weaver.sample-chaos",
        input: {
          ledger: "$.steps.architect.result",
        },
      },
    ],
  })

  expect(result.success).toBe(false)
  expect(result.error?.issues).toContainEqual(
    expect.objectContaining({
      message: 'Step input "ledger" must reference a step output.',
      path: ["steps", 1, "input", "ledger"],
    }),
  )
})

test("PipelineDefinition requires agent prompt instructions", () => {
  expect(() =>
    PipelineDefinition.parse({
      ...validPipeline,
      steps: [
        {
          id: "architect",
          kind: "agent",
          name: "Architect",
        },
      ],
    }),
  ).toThrow("Agent steps require systemPrompt or systemPromptFile.")
})
