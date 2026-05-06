import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { Sigma } from "preact-sigma"

import { goddardSdk } from "~/sdk.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "~/sessions/presentation.ts"
import {
  applySessionChatMessage,
  applySessionChatSession,
  createSessionChatState,
  mergeSessionChatHistory,
  type SessionChatState,
} from "./state.ts"
import { buildSessionChatTranscript } from "./transcript-items.ts"

/** Reactive session chat owner that merges loaded history with live daemon updates. */
export class SessionChat extends Sigma<SessionChatState> {
  constructor(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    super(createSessionChatState(input))
  }

  get hasEmptyTranscript() {
    return this.turns.length === 0 && !this.session.lastAgentMessage
  }

  get transcriptMessages() {
    return buildSessionChatTranscript({
      session: this.session,
      turns: this.turns,
    })
  }

  get repositoryLabel() {
    return getSessionRepositoryLabel(this.session)
  }

  get title() {
    return getSessionDisplayTitle(this.session)
  }

  /** Applies refreshed query data while preserving live messages already received locally. */
  syncLoadedData(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    this.#applyState(mergeSessionChatHistory(this, input))
  }

  /** Applies one live daemon-published ACP message to the chat state. */
  applyMessage(message: acp.AnyMessage) {
    this.#applyState(applySessionChatMessage(this, message))
  }

  /** Applies a freshly returned session record without replacing the merged transcript. */
  syncSession(session: DaemonSession) {
    this.#applyState(applySessionChatSession(this, session))
  }

  onSetup() {
    let active = true
    let unsubscribe: (() => void) | null = null

    void goddardSdk.session
      .subscribe({ id: this.session.id }, (message) => {
        if (active) {
          this.applyMessage(message)
        }
      })
      .then(
        (nextUnsubscribe) => {
          if (active) {
            unsubscribe = nextUnsubscribe
          } else {
            nextUnsubscribe()
          }
        },
        (error) => {
          if (active) {
            console.error("Failed to subscribe to session chat updates.", error)
          }
        },
      )

    return [
      () => {
        active = false
        unsubscribe?.()
        unsubscribe = null
      },
    ]
  }

  #applyState(state: SessionChatState) {
    this.connection = state.connection
    this.hasMore = state.hasMore
    this.nextCursor = state.nextCursor
    this.session = state.session
    this.summary = state.summary
    this.turns = state.turns
  }
}

export interface SessionChat extends SessionChatState {}
