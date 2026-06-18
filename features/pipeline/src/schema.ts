import { z } from "zod"

const identifier = z
  .string()
  .regex(/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/, "Use kebab-case starting with a letter.")

const stepReferencePattern = /^\$\.steps\.([a-z][a-z0-9]*(?:-[a-z0-9]+)*)\.output(?:\.|$)/

export const PipelineStepInputMapping = z.record(
  z.string().min(1),
  z
    .string()
    .min(1)
    .regex(/^\$\.(inputs|steps)\./, "Step input mappings must reference pipeline data."),
)

export type PipelineStepInputMapping = z.infer<typeof PipelineStepInputMapping>

const PipelineStepBase = z.strictObject({
  id: identifier,
  name: z.string().min(1),
  input: PipelineStepInputMapping.default({}),
})

export const PipelineScriptStepDefinition = PipelineStepBase.extend({
  kind: z.literal("script"),
  transformer: z.string().min(1),
})

export type PipelineScriptStepDefinition = z.infer<typeof PipelineScriptStepDefinition>

export const PipelineAgentStepDefinition = PipelineStepBase.extend({
  kind: z.literal("agent"),
  systemPrompt: z.string().min(1).optional(),
  systemPromptFile: z.string().min(1).optional(),
  model: z.string().min(1).optional(),
}).superRefine((step, context) => {
  if (step.systemPrompt || step.systemPromptFile) {
    return
  }

  context.addIssue({
    code: "custom",
    message: "Agent steps require systemPrompt or systemPromptFile.",
    path: ["systemPrompt"],
  })
})

export type PipelineAgentStepDefinition = z.infer<typeof PipelineAgentStepDefinition>

export const PipelineApprovalStepDefinition = PipelineStepBase.extend({
  kind: z.literal("approval"),
  prompt: z.string().min(1).optional(),
})

export type PipelineApprovalStepDefinition = z.infer<typeof PipelineApprovalStepDefinition>

export const PipelineStepDefinition = z.discriminatedUnion("kind", [
  PipelineScriptStepDefinition,
  PipelineAgentStepDefinition,
  PipelineApprovalStepDefinition,
])

export type PipelineStepDefinition = z.infer<typeof PipelineStepDefinition>

export const PipelineDefinition = z
  .strictObject({
    id: identifier,
    version: z.string().min(1),
    name: z.string().min(1),
    description: z.string().optional(),
    inputs: z.record(z.string().min(1), z.unknown()),
    steps: z.array(PipelineStepDefinition).min(1),
    outputs: z.record(z.string().min(1), z.unknown()).optional(),
  })
  .superRefine((definition, context) => {
    const previousStepIds = new Set<string>()
    const seenStepIds = new Set<string>()

    definition.steps.forEach((step, stepIndex) => {
      if (seenStepIds.has(step.id)) {
        context.addIssue({
          code: "custom",
          message: `Duplicate step id "${step.id}".`,
          path: ["steps", stepIndex, "id"],
        })
      }

      seenStepIds.add(step.id)

      for (const [inputName, reference] of Object.entries(step.input)) {
        if (reference.startsWith("$.inputs.")) {
          continue
        }

        const stepReference = reference.match(stepReferencePattern)
        if (!stepReference) {
          context.addIssue({
            code: "custom",
            message: `Step input "${inputName}" must reference a step output.`,
            path: ["steps", stepIndex, "input", inputName],
          })
          continue
        }

        const referencedStepId = stepReference[1]
        if (!previousStepIds.has(referencedStepId)) {
          context.addIssue({
            code: "custom",
            message: `Step input "${inputName}" may only reference earlier step outputs.`,
            path: ["steps", stepIndex, "input", inputName],
          })
        }
      }

      previousStepIds.add(step.id)
    })
  })

export type PipelineDefinition = z.infer<typeof PipelineDefinition>

export const PipelineDefinitionSource = z.enum(["project", "user", "code", "plugin"])

export type PipelineDefinitionSource = z.infer<typeof PipelineDefinitionSource>

export const RegisteredPipelineDefinition = z.strictObject({
  source: PipelineDefinitionSource,
  path: z.string().min(1).optional(),
  definition: PipelineDefinition,
})

export type RegisteredPipelineDefinition = z.infer<typeof RegisteredPipelineDefinition>

export const PipelineDefinitionDiagnostic = z.strictObject({
  source: PipelineDefinitionSource,
  path: z.string().min(1).optional(),
  message: z.string().min(1),
})

export type PipelineDefinitionDiagnostic = z.infer<typeof PipelineDefinitionDiagnostic>

export const ListPipelineDefinitionsRequest = z.strictObject({
  cwd: z.string().min(1),
})

export type ListPipelineDefinitionsRequest = z.infer<typeof ListPipelineDefinitionsRequest>

export type ListPipelineDefinitionsResponse = {
  definitions: RegisteredPipelineDefinition[]
  diagnostics: PipelineDefinitionDiagnostic[]
}

export type ListPipelineDefinitionDiagnosticsResponse = {
  diagnostics: PipelineDefinitionDiagnostic[]
}

export const PipelineRunId = z.custom<`plr_${string}`>(
  (value): value is `plr_${string}` => typeof value === "string" && value.startsWith("plr_"),
)

export type PipelineRunId = z.infer<typeof PipelineRunId>

export const PipelineStepRunId = z.custom<`pls_${string}`>(
  (value): value is `pls_${string}` => typeof value === "string" && value.startsWith("pls_"),
)

export type PipelineStepRunId = z.infer<typeof PipelineStepRunId>

export const PipelineAgentSessionId = z.custom<`ses_${string}`>(
  (value): value is `ses_${string}` => typeof value === "string" && value.startsWith("ses_"),
)

export type PipelineAgentSessionId = z.infer<typeof PipelineAgentSessionId>

export const PipelineRunStatus = z.enum([
  "queued",
  "running",
  "waiting",
  "succeeded",
  "failed",
  "cancelled",
])

export type PipelineRunStatus = z.infer<typeof PipelineRunStatus>

export const PipelineRunOrigin = z.enum(["app", "sdk", "cli", "automation"])

export type PipelineRunOrigin = z.infer<typeof PipelineRunOrigin>

export const PipelineRunVisibility = z.enum(["visible", "hidden"])

export type PipelineRunVisibility = z.infer<typeof PipelineRunVisibility>

export const PipelineStepRunStatus = z.enum([
  "queued",
  "running",
  "waiting",
  "succeeded",
  "failed",
  "skipped",
])

export type PipelineStepRunStatus = z.infer<typeof PipelineStepRunStatus>

export const PipelineRunInputs = z.record(z.string().min(1), z.unknown())

export type PipelineRunInputs = z.infer<typeof PipelineRunInputs>

export const PipelineRunOutputs = z.record(z.string().min(1), z.unknown()).nullable()

export type PipelineRunOutputs = z.infer<typeof PipelineRunOutputs>

export const PipelineStepRunData = z.unknown().nullable()

export type PipelineStepRunData = z.infer<typeof PipelineStepRunData>

export const PipelineRunDefinitionSnapshot = z.strictObject({
  definition: PipelineDefinition,
  source: PipelineDefinitionSource,
  path: z.string().min(1).optional(),
})

export type PipelineRunDefinitionSnapshot = z.infer<typeof PipelineRunDefinitionSnapshot>

export const DaemonPipelineRun = z.strictObject({
  pipelineId: z.string().min(1),
  pipelineVersion: z.string().min(1),
  cwd: z.string().min(1),
  status: PipelineRunStatus,
  origin: PipelineRunOrigin,
  visibility: PipelineRunVisibility,
  inputs: PipelineRunInputs,
  outputs: PipelineRunOutputs,
  error: z.string().nullable(),
  definitionSnapshot: PipelineRunDefinitionSnapshot,
  startedAt: z.number().int().nullable(),
  completedAt: z.number().int().nullable(),
  updatedAt: z.number().int(),
})

export type DaemonPipelineRun = z.infer<typeof DaemonPipelineRun> & {
  id: PipelineRunId
  createdAt: number
}

export const DaemonPipelineStepRun = z.strictObject({
  pipelineRunId: PipelineRunId,
  stepId: z.string().min(1),
  stepIndex: z.number().int().nonnegative(),
  kind: z.enum(["script", "agent", "approval"]),
  status: PipelineStepRunStatus,
  input: PipelineRunInputs,
  output: PipelineStepRunData,
  error: z.string().nullable(),
  sessionId: PipelineAgentSessionId.nullable(),
  stopReason: z.string().nullable(),
  startedAt: z.number().int().nullable(),
  completedAt: z.number().int().nullable(),
  updatedAt: z.number().int(),
})

export type DaemonPipelineStepRun = z.infer<typeof DaemonPipelineStepRun> & {
  id: PipelineStepRunId
  createdAt: number
}

export const SpawnPipelineRunRequest = z.strictObject({
  cwd: z.string().min(1),
  pipelineId: z.string().min(1),
  pipelineVersion: z.string().min(1).optional(),
  inputs: PipelineRunInputs,
  origin: PipelineRunOrigin.default("sdk"),
  visibility: PipelineRunVisibility.default("visible"),
})

export type SpawnPipelineRunRequest = z.infer<typeof SpawnPipelineRunRequest>

export const GetPipelineRunRequest = z.strictObject({
  id: PipelineRunId,
})

export type GetPipelineRunRequest = z.infer<typeof GetPipelineRunRequest>

export const ListPipelineRunsRequest = z.strictObject({
  pipelineId: z.string().min(1).optional(),
  limit: z.number().int().positive().max(100).optional(),
  cursor: z.string().optional().nullable(),
})

export type ListPipelineRunsRequest = z.infer<typeof ListPipelineRunsRequest>

export const CancelPipelineRunRequest = GetPipelineRunRequest

export type CancelPipelineRunRequest = z.infer<typeof CancelPipelineRunRequest>

export const AdvancePipelineRunRequest = GetPipelineRunRequest

export type AdvancePipelineRunRequest = z.infer<typeof AdvancePipelineRunRequest>

export const ApprovePipelineRunRequest = GetPipelineRunRequest

export type ApprovePipelineRunRequest = z.infer<typeof ApprovePipelineRunRequest>

export const RetryPipelineRunRequest = GetPipelineRunRequest

export type RetryPipelineRunRequest = z.infer<typeof RetryPipelineRunRequest>

export type PipelineRunWithSteps = {
  run: DaemonPipelineRun
  steps: DaemonPipelineStepRun[]
}

export type SpawnPipelineRunResponse = PipelineRunWithSteps

export type GetPipelineRunResponse = PipelineRunWithSteps

export type ListPipelineRunsResponse = {
  runs: DaemonPipelineRun[]
  nextCursor: string | null
  hasMore: boolean
}

export type CancelPipelineRunResponse = PipelineRunWithSteps

export type AdvancePipelineRunResponse = PipelineRunWithSteps

export type ApprovePipelineRunResponse = PipelineRunWithSteps

export type RetryPipelineRunResponse = PipelineRunWithSteps
