import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  FileSearchComposerEntriesRequest,
  type FileSearchComposerEntriesResponse,
} from "./schema.ts"

export const fileSearchIpcRoutes = defineIpcRoutes({
  fileSearch: http.resource("file-search", {
    /** Finds file and folder entries for composer `@` suggestions. */
    composerEntries: http.post("composer-entries", {
      body: FileSearchComposerEntriesRequest,
      response: $type<FileSearchComposerEntriesResponse>(),
    }),
  }),
})
