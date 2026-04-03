import { SigmaType } from "preact-sigma"
import type { SessionChat } from "~/session-chat/chat.ts"
import type { SessionIndex } from "./session-index.ts"
import type { SessionService } from "./session-service.ts"

type SessionLaunchShape = {
  isDialogOpen: boolean
  draftProjectPath: string | null
  draftPrompt: string
  submitStatus: "idle" | "submitting" | "error"
  errorMessage: string | null
}

export const SessionLaunch = new SigmaType<SessionLaunchShape>("SessionLaunch")
  .defaultState({
    isDialogOpen: false,
    draftProjectPath: null,
    draftPrompt: "",
    submitStatus: "idle",
    errorMessage: null,
  })
  .queries({
    canSubmit() {
      return this.draftProjectPath !== null && this.draftPrompt.trim().length > 0
    },
  })
  .actions({
    openDialog(preferredProjectPath?: string | null) {
      this.isDialogOpen = true
      this.draftProjectPath = preferredProjectPath ?? null
      this.draftPrompt = ""
      this.submitStatus = "idle"
      this.errorMessage = null
    },

    closeDialog() {
      this.isDialogOpen = false
      this.draftProjectPath = null
      this.draftPrompt = ""
      this.submitStatus = "idle"
      this.errorMessage = null
    },

    setDraftProjectPath(projectPath: string | null) {
      this.draftProjectPath = projectPath
    },

    setDraftPrompt(prompt: string) {
      this.draftPrompt = prompt
    },

    beginSubmit() {
      this.submitStatus = "submitting"
      this.errorMessage = null
    },

    failSubmit(message: string) {
      this.submitStatus = "error"
      this.errorMessage = message
    },

    async submitLaunch(
      service: SessionService,
      sessionIndex: SessionIndex,
      sessionChat: SessionChat,
    ) {
      if (!this.canSubmit()) {
        this.failSubmit("Choose a project and enter the first prompt.")
        return null
      }

      this.beginSubmit()
      this.commit()

      try {
        const session = await sessionIndex.createSession(service, {
          agent: "pi",
          cwd: this.draftProjectPath!,
          mcpServers: [],
          systemPrompt: "",
          initialPrompt: this.draftPrompt.trim(),
        })
        await sessionChat.loadThread(service, session)
        this.closeDialog()
        this.commit()
        return session
      } catch (error) {
        this.failSubmit(error instanceof Error ? error.message : String(error))
        this.commit()
        return null
      }
    },
  })

export interface SessionLaunch extends InstanceType<typeof SessionLaunch> {}
