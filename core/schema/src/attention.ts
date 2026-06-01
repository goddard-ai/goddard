import { z } from "zod"

/** Short semi-stable subject label for daemon attention events and projections. */
export const AttentionScope = z.string().trim().min(1).max(80)

export type AttentionScope = z.infer<typeof AttentionScope>

/** Short turn-specific preview text for daemon attention events and projections. */
export const AttentionHeadline = z.string().trim().min(1).max(120)

export type AttentionHeadline = z.infer<typeof AttentionHeadline>

/** Optional agent-supplied attention metadata attached to daemon workflow reporting. */
export const AttentionMetadataInput = z.strictObject({
  scope: AttentionScope.optional(),
  headline: AttentionHeadline.optional(),
})

export type AttentionMetadataInput = z.infer<typeof AttentionMetadataInput>
