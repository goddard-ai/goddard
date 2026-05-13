import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"

import type { SessionChat } from "./model.ts"
import { createQueryDataContext } from "~/lib/query-data-context.tsrx"

export const {
  QueryDataProvider: SessionChatQueryDataProvider,
  useQueryData: useSessionChatQueryData,
} = createQueryDataContext<
  {
    chat: SessionChat;
    session: DaemonSession;
    sessionHistory: GetSessionHistoryResponse;
  }
>()
