import { mergeConfigLayers, selectLast } from "@goddard-ai/config"
import { InlineSessionParams, StaticSessionParams } from "@goddard-ai/schema/config"
import { z } from "zod"

/** Persisted settings for named action packages and root action defaults. */
export const ActionConfig = z.strictObject({
  session: StaticSessionParams.optional().describe(
    "Default session settings applied to named agent actions.",
  ),
})

export type ActionConfig = z.infer<typeof ActionConfig>

/** Merges action config layers using later layers as overrides. */
export function mergeActionConfigLayers(...layers: Array<ActionConfig | undefined>) {
  const merged = mergeConfigLayers<ActionConfig>(layers)

  return ActionConfig.parse({
    ...merged,
    session: selectLast(layers, (layer) => layer?.session),
  })
}

/** Request payload used to run one named daemon-resolved action. */
export const RunNamedActionRequest = InlineSessionParams.extend({
  actionName: z.string().min(1),
  cwd: z.string().min(1),
})

export type RunNamedActionRequest = z.infer<typeof RunNamedActionRequest>
