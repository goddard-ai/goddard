import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import type { Protected } from "preact-sigma"

import type { SessionChat } from "./model.ts"
import { createQueryDataContext } from "~/lib/query-data-context.tsrx"

export const {
  QueryDataProvider: SessionChatQueryDataProvider,
  useQueryData: useSessionChatQueryData,
} = createQueryDataContext<
  {
    chat: Protected<SessionChat>;
    session: DaemonSession;
    sessionHistory: GetSessionHistoryResponse;
  }
>()
