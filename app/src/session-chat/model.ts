import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import { Sigma } from "preact-sigma"

import { goddardSdk } from "~/sdk.ts"
import { SessionChat } from "./chat.ts"
import {
  applySessionChatMessage,
  createSessionChatState,
  mergeSessionChatHistory,
  type SessionChatState,
} from "./state.ts"

/** Reactive session chat model that owns live daemon subscription wiring. */
export type SessionChatModelState = SessionChatState

/** Session chat Sigma wrapper around the pure state reducer. */
export class SessionChatModel extends Sigma<SessionChatModelState> {
  #transcript: SessionChat

  constructor(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    const state = createSessionChatState(input)

    super(state)
    this.#transcript = new SessionChat({
      session: state.session,
      turns: state.turns,
    })
  }

  get hasEmptyTranscript() {
    return this.turns.length === 0 && !this.session.lastAgentMessage
  }

  get transcriptMessages() {
    return this.#transcript.messages
  }

  /** Applies refreshed query data while preserving live messages already received locally. */
  syncLoadedData(input: { history: GetSessionHistoryResponse; session: DaemonSession }) {
    this.#applyState(mergeSessionChatHistory(this, input))
  }

  /** Applies one live daemon-published ACP message to the chat state. */
  applyMessage(message: acp.AnyMessage) {
    this.#applyState(applySessionChatMessage(this, message))
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
    this.#transcript.loadTranscript({
      session: state.session,
      turns: state.turns,
    })
  }
}

export interface SessionChatModel extends SessionChatModelState {}
