import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"

import type { SessionChat } from "./model.ts"
import { createPageQueryDataContext } from "~/lib/page-query-data-context.tsrx"

export const {
  PageQueryDataProvider: SessionChatQueryDataProvider,
  usePageQueryData: useSessionChatQueryData,
} = createPageQueryDataContext<
  {
    chat: SessionChat;
    session: DaemonSession;
    sessionHistory: GetSessionHistoryResponse;
  }
>()
