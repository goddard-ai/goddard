import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"

import {
  FileSearchComposerEntriesRequest,
  type FileSearchComposerEntriesResponse,
} from "./schema.ts"

export const fileSearchIpcRoutes = defineIpcRoutes({
  fileSearch: http.resource("file-search", {
    ...ipcMetadata({
      description: "Project file and folder search.",
    }),
    /** Finds file and folder entries for composer `@` suggestions. */
    composerEntries: http.post("composer-entries", {
      ...ipcMetadata({
        description: "Finds file and folder entries for composer `@` suggestions.",
      }),
      body: FileSearchComposerEntriesRequest,
      response: $type<FileSearchComposerEntriesResponse>(),
    }),
  }),
})
