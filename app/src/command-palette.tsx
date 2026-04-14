/** Renders the app-wide command palette. */
import { Dialog, UseDialogReturn } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Command } from "ark-cmdk"
import { Search } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import { useEffect, useState } from "preact/hooks"

import type { AppCommandId } from "~/shared/app-commands.ts"
import { AppCommand, appCommandList } from "./commands/app-command.ts"

/** One actionable item shown in the command palette. */
type CommandMenuItem = {
  command: AppCommand
  id: AppCommandId
  icon: LucideIcon
  keywords?: readonly string[]
  label: string
}

export default function CommandPalette(props: { dialog: UseDialogReturn }) {
  const { open } = props.dialog

  const [search, setSearch] = useState("")
  useEffect(() => {
    if (!open && search.length > 0) {
      setSearch("")
    }
  }, [open, search])

  return (
    <Portal>
      <Dialog.Root>
        <Dialog.Backdrop
          class={css({
            position: "fixed",
            inset: "0",
            backgroundColor: "overlay",
            backdropFilter: "blur(10px)",
            opacity: "1",
            transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
            zIndex: "50",
            "@starting-style": {
              opacity: "0",
            },
          })}
        />
        <Dialog.Positioner
          class={css({
            position: "fixed",
            inset: "0",
            display: "grid",
            justifyItems: "center",
            alignContent: "start",
            paddingTop: "14vh",
            paddingInline: "12px",
            zIndex: "51",
          })}
        >
          <Dialog.Content
            class={css({
              width: "min(560px, calc(100vw - 24px))",
              maxHeight: "min(460px, calc(100vh - 32px))",
              padding: "0",
              display: "flex",
              flexDirection: "column",
              border: "1px solid",
              borderColor: "border",
              borderRadius: "20px",
              backgroundColor: "panel",
              boxShadow: `0 24px 56px ${token.var("colors.shadow")}`,
              overflow: "hidden",
              outline: "none",
              opacity: "1",
              transform: "translateY(0) scale(1)",
              transition:
                "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
              "@starting-style": {
                opacity: "0",
                transform: "translateY(12px) scale(0.985)",
              },
            })}
          >
            <Dialog.Title
              class={css({
                position: "absolute",
                width: "1px",
                height: "1px",
                padding: "0",
                margin: "-1px",
                overflow: "hidden",
                clip: "rect(0, 0, 0, 0)",
                whiteSpace: "nowrap",
                border: "0",
              })}
            >
              Command menu
            </Dialog.Title>
            <Command.Root<AppCommand>
              class={css({
                display: "flex",
                flexDirection: "column",
                minHeight: "0",
                width: "100%",
                background: "transparent",
                color: "text",
              })}
              itemToKeywords={(command) => command.keywords}
              itemToString={(command) => command.label}
              itemToValue={(command) => command.id}
              items={appCommandList}
              label="Command menu"
              onSearchChange={setSearch}
              onSelect={(command) => {
                props.dialog.setOpen(false)
                command()
              }}
              search={search}
            >
              <div
                class={css({
                  display: "flex",
                  alignItems: "center",
                  gap: "10px",
                  minHeight: "56px",
                  paddingInline: "16px",
                  borderBottom: "1px solid",
                  borderColor: "border",
                  backgroundColor: "background",
                  transition:
                    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                  _focusWithin: {
                    backgroundColor: "surface",
                    borderColor: "accent",
                  },
                })}
              >
                <Search
                  aria-hidden={true}
                  class={css({
                    flexShrink: "0",
                    color: "muted",
                  })}
                  size={18}
                />
                <Command.Input
                  autoFocus={true}
                  class={css({
                    width: "100%",
                    height: "28px",
                    border: "none",
                    outline: "none",
                    backgroundColor: "transparent",
                    color: "text",
                    fontSize: "0.94rem",
                    fontWeight: "600",
                    letterSpacing: "-0.01em",
                    "&::placeholder": {
                      color: "muted",
                    },
                  })}
                  placeholder="Type a command or jump to a view"
                />
              </div>
              <Command.List<CommandMenuItem>
                class={css({
                  padding: "8px",
                  overflowY: "auto",
                  "& [data-scope='combobox'][data-part='item']": {
                    outline: "none",
                  },
                  "& [data-scope='combobox'][data-part='item'] + [data-scope='combobox'][data-part='item']":
                    {
                      marginTop: "4px",
                    },
                  "& [data-scope='combobox'][data-part='item']:focus-visible > div": {
                    outline: "2px solid",
                    outlineColor: "accentStrong",
                    outlineOffset: "2px",
                  },
                })}
                empty={
                  <div
                    class={css({
                      display: "grid",
                      placeItems: "center",
                      minHeight: "88px",
                      color: "muted",
                      fontSize: "0.86rem",
                      fontWeight: "600",
                    })}
                  >
                    No matching commands.
                  </div>
                }
              >
                {(item, state) => {
                  const Icon = item.icon

                  return (
                    <div
                      class={css({
                        display: "grid",
                        gridTemplateColumns: "auto minmax(0, 1fr) auto",
                        alignItems: "center",
                        gap: "10px",
                        width: "100%",
                        minHeight: "44px",
                        padding: "8px 10px",
                        border: "1px solid",
                        borderColor: "transparent",
                        borderRadius: "14px",
                        backgroundColor: "transparent",
                        color: "text",
                        cursor: "pointer",
                        transition:
                          "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
                        "&[data-active='true']": {
                          backgroundColor: "surface",
                          borderColor: "accent",
                          boxShadow: `0 12px 28px ${token.var("colors.shadow")}`,
                        },
                      })}
                      data-active={state.active ? "true" : "false"}
                    >
                      <span
                        class={css({
                          display: "grid",
                          placeItems: "center",
                          width: "30px",
                          height: "30px",
                          borderRadius: "10px",
                          border: "1px solid",
                          borderColor: "border",
                          backgroundColor: "background",
                          color: "accentStrong",
                          transition:
                            "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                          "&[data-active='true']": {
                            borderColor: "accent",
                            backgroundColor: "accent",
                            color: "accentFg",
                          },
                        })}
                        data-active={state.active ? "true" : "false"}
                      >
                        <Icon aria-hidden={true} size={16} strokeWidth={2} />
                      </span>
                      <span
                        class={css({
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          fontSize: "0.9rem",
                          fontWeight: "620",
                        })}
                      >
                        {item.label}
                      </span>
                      {item.shortcut ? (
                        <span
                          class={css({
                            paddingInline: "8px",
                            height: "20px",
                            borderRadius: "999px",
                            border: "1px solid",
                            borderColor: "border",
                            backgroundColor: "background",
                            color: "muted",
                            fontSize: "0.68rem",
                            fontWeight: "700",
                            letterSpacing: "0.04em",
                            lineHeight: "20px",
                          })}
                        >
                          {item.shortcut}
                        </span>
                      ) : null}
                    </div>
                  )
                }}
              </Command.List>
            </Command.Root>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>
    </Portal>
  )
}
