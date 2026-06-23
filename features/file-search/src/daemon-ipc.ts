import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"

import {
  FileSearchComposerEntriesRequest,
  type FileSearchComposerEntriesResponse,
} from "./schema.ts"

export const fileSearchIpcRoutes = defineIpcRoutes({
  fileSearch: http.resource("file-search", {
    ...metadata({
      description: "Project file and folder search.",
    }),
    /** Finds file and folder entries for composer `@` suggestions. */
    composerEntries: http.post("composer-entries", {
      ...metadata({
        description: "Finds file and folder entries for composer `@` suggestions.",
      }),
      body: FileSearchComposerEntriesRequest,
      response: $type<FileSearchComposerEntriesResponse>(),
    }),
  }),
})
