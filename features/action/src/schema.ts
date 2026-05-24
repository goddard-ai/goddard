import { InlineSessionParams, StaticSessionParams } from "@goddard-ai/schema/config"
import { z } from "zod"

/** Persisted settings for named action packages and root action defaults. */
export const ActionConfig = z.strictObject({
  session: StaticSessionParams.optional().describe(
    "Default session settings applied to named agent actions.",
  ),
})

export type ActionConfig = z.infer<typeof ActionConfig>

/** Request payload used to run one named daemon-resolved action. */
export const RunNamedActionRequest = InlineSessionParams.extend({
  actionName: z.string().min(1),
  cwd: z.string().min(1),
})

export type RunNamedActionRequest = z.infer<typeof RunNamedActionRequest>
