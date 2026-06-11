import { z } from "zod"

/** Request payload used to find file and folder entries for composer `@` suggestions. */
export const FileSearchComposerEntriesRequest = z.strictObject({
  cwd: z.string().min(1),
  query: z.string(),
  limit: z.number().int().positive().optional(),
})

export type FileSearchComposerEntriesRequest = z.infer<typeof FileSearchComposerEntriesRequest>

/** One file or folder entry returned to app composers for `@` suggestions. */
export type FileSearchComposerEntry = {
  type: "file" | "folder"
  path: string
  uri: string
  label: string
  detail: string
}

/** Response payload for composer file and folder entry search. */
export type FileSearchComposerEntriesResponse = {
  entries: FileSearchComposerEntry[]
}
