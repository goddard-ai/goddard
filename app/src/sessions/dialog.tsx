import * as RadixDialog from "@radix-ui/react-dialog"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Sparkles, X } from "lucide-react"
import {
  useProjectRegistry,
  useSessionChat,
  useSessionIndex,
  useSessionLaunch,
  useWorkbenchTabSet,
} from "~/app-state-context.tsx"
import { LaunchForm } from "./launch-form.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"
import { desktopSessionService } from "./session-service.ts"

export function Dialog() {
  const projectRegistry = useProjectRegistry()
  const sessionChat = useSessionChat()
  const sessionIndex = useSessionIndex()
  const sessionLaunch = useSessionLaunch()
  const workbenchTabSet = useWorkbenchTabSet()

  function closeDialog() {
    sessionLaunch.closeDialog()
  }

  async function launchSession() {
    const session = await sessionLaunch.submitLaunch(
      desktopSessionService,
      sessionIndex,
      sessionChat,
    )

    if (!session) {
      return
    }

    workbenchTabSet.openOrFocusTab({
      id: `session:${session.id}`,
      kind: "sessionChat",
      title: getSessionDisplayTitle(session),
      payload: {
        sessionId: session.id,
      },
      dirty: false,
    })
  }

  return (
    <RadixDialog.Root
      open={sessionLaunch.isDialogOpen}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          closeDialog()
        }
      }}
    >
      <RadixDialog.Portal>
        <RadixDialog.Overlay
          class={css({
            position: "fixed",
            inset: "0",
            background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 10%, rgba(23, 29, 36, 0.24)), rgba(23, 29, 36, 0.34))`,
            backdropFilter: "blur(10px)",
            opacity: "1",
            transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
            "@starting-style": {
              opacity: "0",
            },
          })}
        />
        <RadixDialog.Content
          class={css({
            position: "fixed",
            top: "50%",
            left: "50%",
            width: "min(640px, calc(100vw - 32px))",
            maxHeight: "calc(100vh - 32px)",
            overflowY: "auto",
            padding: "28px",
            borderRadius: "30px",
            border: "1px solid",
            borderColor: "border",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            boxShadow: "0 34px 90px rgba(84, 102, 124, 0.2)",
            transform: "translate(-50%, -50%)",
            transition:
              "opacity 220ms cubic-bezier(0.23, 1, 0.32, 1), transform 220ms cubic-bezier(0.23, 1, 0.32, 1)",
            "@starting-style": {
              opacity: "0",
              transform: "translate(-50%, calc(-50% + 16px)) scale(0.985)",
            },
          })}
        >
          <div
            class={css({
              display: "flex",
              alignItems: "flex-start",
              justifyContent: "space-between",
              gap: "16px",
              marginBottom: "22px",
            })}
          >
            <div class={css({ display: "grid", gap: "10px" })}>
              <span
                class={css({
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "8px",
                  width: "fit-content",
                  padding: "8px 12px",
                  borderRadius: "999px",
                  backgroundColor: "surface",
                  color: "accentStrong",
                  fontSize: "0.72rem",
                  fontWeight: "700",
                  letterSpacing: "0.14em",
                  textTransform: "uppercase",
                })}
              >
                <Sparkles size={14} strokeWidth={2} />
                New session
              </span>
              <div class={css({ display: "grid", gap: "8px" })}>
                <RadixDialog.Title
                  class={css({
                    color: "text",
                    fontSize: "1.45rem",
                    fontWeight: "760",
                    letterSpacing: "-0.03em",
                    lineHeight: "1.1",
                  })}
                >
                  Launch a local session
                </RadixDialog.Title>
                <RadixDialog.Description
                  class={css({
                    color: "muted",
                    fontSize: "0.94rem",
                    lineHeight: "1.7",
                  })}
                >
                  This minimal flow creates a session record, seeds the transcript, and opens the
                  chat tab immediately.
                </RadixDialog.Description>
              </div>
            </div>
            <RadixDialog.Close asChild>
              <button
                class={cx(
                  css({
                    display: "grid",
                    placeItems: "center",
                    width: "36px",
                    height: "36px",
                    borderRadius: "12px",
                    border: "1px solid",
                    borderColor: "border",
                    backgroundColor: "background",
                    color: "muted",
                    cursor: "pointer",
                  }),
                )}
                type="button"
              >
                <X size={16} strokeWidth={2.2} />
              </button>
            </RadixDialog.Close>
          </div>
          <LaunchForm
            canSubmit={sessionLaunch.canSubmit()}
            draftProjectPath={sessionLaunch.draftProjectPath}
            draftPrompt={sessionLaunch.draftPrompt}
            errorMessage={sessionLaunch.errorMessage}
            onChangeProjectPath={(projectPath) => {
              sessionLaunch.setDraftProjectPath(projectPath)
            }}
            onChangePrompt={(prompt) => {
              sessionLaunch.setDraftPrompt(prompt)
            }}
            onSubmit={launchSession}
            projects={projectRegistry.projectList}
            submitStatus={sessionLaunch.submitStatus}
          />
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  )
}
