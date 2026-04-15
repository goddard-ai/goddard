import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { useQueries } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/actions.ts"
import { buildTranscriptMessages } from "./chat.ts"
import { Composer } from "./composer.tsx"
import { Header } from "./header.tsx"
import { Transcript } from "./transcript.tsx"

export function SessionChatView(props: { sessionId: string }) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const sessionId = props.sessionId as DaemonSession["id"]
  const [history, { session }] = useQueries([
    [goddardSdk.session.history, [{ id: sessionId }]],
    [goddardSdk.session.get, [{ id: sessionId }]],
  ])
  const messages = buildTranscriptMessages(session, history.turns)
  const resolvedProjectPath = findNearestProjectPath(projectRegistry.projectList, session.cwd)

  useEffect(() => {
    projectContext.reportTabProject(workbenchTabSet.activeTabId, resolvedProjectPath)

    return () => {
      projectContext.clearTabProject(workbenchTabSet.activeTabId)
    }
  }, [projectContext, resolvedProjectPath, workbenchTabSet.activeTabId])

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateRows: "auto minmax(0, 1fr) auto",
        height: "100%",
      })}
    >
      <Header messageCount={messages.length} session={session} />
      <Transcript
        initialScrollPosition="bottom"
        messages={messages}
        scrollCacheKey={`detail:session:${session.id}:transcript`}
      />
      <Composer
        sessionId={session.id}
        onSubmit={async (prompt) => {
          try {
            await submitSessionPrompt({
              id: session.id,
              acpId: session.acpSessionId,
              prompt,
            })
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
          }
        }}
      />
    </div>
  )
}

export default SessionChatView
