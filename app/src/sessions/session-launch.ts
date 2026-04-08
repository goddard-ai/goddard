import { SigmaType } from "preact-sigma"
import type { CreateDaemonSessionRequest } from "@goddard-ai/sdk"

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
    createSessionInput() {
      const prompt = this.draftPrompt.trim()

      if (!this.draftProjectPath || prompt.length === 0) {
        return null
      }

      return {
        agent: "pi",
        cwd: this.draftProjectPath,
        mcpServers: [],
        systemPrompt: "",
        initialPrompt: prompt,
      } satisfies CreateDaemonSessionRequest
    },

    canSubmit() {
      return this.createSessionInput() !== null
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
  })

export interface SessionLaunch extends InstanceType<typeof SessionLaunch> {}
