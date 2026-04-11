import * as Dialog from "@radix-ui/react-dialog"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Monitor, Moon, Sun, X } from "lucide-react"
import { useAppearance } from "~/app-state-context.tsx"
import type { AppearanceMode } from "./theme.ts"

const themeModeOptions = [
  {
    mode: "system",
    label: "System",
    description: "Follow the operating system appearance.",
    icon: Monitor,
  },
  {
    mode: "light",
    label: "Light",
    description: "Use the light built-in palette.",
    icon: Sun,
  },
  {
    mode: "dark",
    label: "Dark",
    description: "Use the dark built-in palette.",
    icon: Moon,
  },
] as const satisfies {
  mode: AppearanceMode
  label: string
  description: string
  icon: typeof Monitor
}[]

export function AppearanceDialog(props: {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
}) {
  const appearance = useAppearance()

  return (
    <Dialog.Root open={props.isOpen} onOpenChange={props.onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay
          class={css({
            position: "fixed",
            inset: "0",
            background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 16%, ${token.var("colors.overlay")}), ${token.var("colors.overlay")})`,
            backdropFilter: "blur(12px)",
            opacity: "1",
            transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
            "@starting-style": {
              opacity: "0",
            },
          })}
        />
        <Dialog.Content
          class={css({
            position: "fixed",
            top: "50%",
            left: "50%",
            width: "min(560px, calc(100vw - 32px))",
            maxHeight: "calc(100vh - 32px)",
            overflowY: "auto",
            padding: "28px",
            borderRadius: "28px",
            border: "1px solid",
            borderColor: "border",
            background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
            boxShadow: `0 34px 90px ${token.var("colors.shadow")}`,
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
              marginBottom: "24px",
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
                Appearance
              </span>
              <div class={css({ display: "grid", gap: "8px" })}>
                <Dialog.Title
                  class={css({
                    color: "text",
                    fontSize: "1.45rem",
                    fontWeight: "760",
                    letterSpacing: "-0.03em",
                    lineHeight: "1.08",
                  })}
                >
                  Theme the workbench without exposing every token
                </Dialog.Title>
                <Dialog.Description
                  class={css({
                    color: "muted",
                    fontSize: "0.94rem",
                    lineHeight: "1.72",
                  })}
                >
                  Built-in themes stay seed-derived under the hood. The UI keeps consuming the same
                  semantic color roles either way.
                </Dialog.Description>
              </div>
            </div>
            <Dialog.Close asChild>
              <button
                class={css({
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
                  transition:
                    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                  _hover: {
                    backgroundColor: "surface",
                    color: "text",
                  },
                })}
                type="button"
              >
                <X size={16} strokeWidth={2.2} />
              </button>
            </Dialog.Close>
          </div>
          <div class={css({ display: "grid", gap: "18px" })}>
            <section class={css({ display: "grid", gap: "10px" })}>
              <div class={css({ display: "grid", gap: "4px" })}>
                <h2
                  class={css({
                    margin: "0",
                    color: "text",
                    fontSize: "1rem",
                    fontWeight: "720",
                    letterSpacing: "-0.02em",
                  })}
                >
                  Color mode
                </h2>
                <p
                  class={css({
                    margin: "0",
                    color: "muted",
                    fontSize: "0.88rem",
                    lineHeight: "1.7",
                  })}
                >
                  Current effective theme: {appearance.effectiveTheme}
                </p>
              </div>
              <div
                class={css({
                  display: "grid",
                  gap: "10px",
                })}
                role="radiogroup"
                aria-label="Theme mode"
              >
                {themeModeOptions.map((option) => {
                  const Icon = option.icon
                  const isSelected = appearance.mode === option.mode

                  return (
                    <button
                      key={option.mode}
                      aria-checked={isSelected}
                      class={cx(
                        css({
                          display: "grid",
                          gridTemplateColumns: "auto minmax(0, 1fr) auto",
                          alignItems: "center",
                          gap: "14px",
                          width: "100%",
                          padding: "14px 16px",
                          borderRadius: "18px",
                          border: "1px solid",
                          borderColor: "border",
                          background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
                          color: "text",
                          cursor: "pointer",
                          textAlign: "left",
                          transition:
                            "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                          _active: {
                            transform: "scale(0.994)",
                          },
                          _focusVisible: {
                            outline: "2px solid",
                            outlineColor: "accentStrong",
                            outlineOffset: "2px",
                          },
                          "&[data-selected='true']": {
                            borderColor: "accent",
                            boxShadow: `0 18px 36px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent)`,
                          },
                        }),
                      )}
                      data-selected={isSelected}
                      role="radio"
                      type="button"
                      onClick={() => {
                        appearance.setMode(option.mode)
                      }}
                    >
                      <span
                        class={css({
                          display: "grid",
                          placeItems: "center",
                          width: "38px",
                          height: "38px",
                          borderRadius: "14px",
                          backgroundColor: "panel",
                          color: "accentStrong",
                        })}
                      >
                        <Icon size={18} strokeWidth={2.1} />
                      </span>
                      <span class={css({ display: "grid", gap: "4px", minWidth: "0" })}>
                        <span
                          class={css({
                            color: "text",
                            fontSize: "0.96rem",
                            fontWeight: "700",
                            letterSpacing: "-0.01em",
                          })}
                        >
                          {option.label}
                        </span>
                        <span
                          class={css({
                            color: "muted",
                            fontSize: "0.83rem",
                            lineHeight: "1.6",
                          })}
                        >
                          {option.description}
                        </span>
                      </span>
                      <span
                        aria-hidden="true"
                        class={css({
                          width: "12px",
                          height: "12px",
                          borderRadius: "999px",
                          border: "1px solid",
                          borderColor: "border",
                          backgroundColor: "background",
                          transition:
                            "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                          "&[data-selected='true']": {
                            borderColor: "accentStrong",
                            backgroundColor: "accentStrong",
                          },
                        })}
                        data-selected={isSelected}
                      />
                    </button>
                  )
                })}
              </div>
            </section>
            <section
              class={css({
                display: "grid",
                gap: "12px",
                padding: "18px",
                borderRadius: "22px",
                border: "1px solid",
                borderColor: "border",
                background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
              })}
            >
              <div class={css({ display: "grid", gap: "4px" })}>
                <h2
                  class={css({
                    margin: "0",
                    color: "text",
                    fontSize: "1rem",
                    fontWeight: "720",
                    letterSpacing: "-0.02em",
                  })}
                >
                  High contrast
                </h2>
                <p
                  class={css({
                    margin: "0",
                    color: "muted",
                    fontSize: "0.85rem",
                    lineHeight: "1.7",
                  })}
                >
                  Tightens muted text and border separation without exposing a freeform slider.
                </p>
              </div>
              <button
                aria-pressed={appearance.highContrast}
                class={css({
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: "16px",
                  width: "100%",
                  padding: "14px 16px",
                  borderRadius: "18px",
                  border: "1px solid",
                  borderColor: "border",
                  backgroundColor: "background",
                  color: "text",
                  cursor: "pointer",
                  transition:
                    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1), background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                  _focusVisible: {
                    outline: "2px solid",
                    outlineColor: "accentStrong",
                    outlineOffset: "2px",
                  },
                  "&[data-enabled='true']": {
                    borderColor: "accent",
                    boxShadow: `0 18px 36px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent)`,
                    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
                  },
                })}
                data-enabled={appearance.highContrast}
                type="button"
                onClick={() => {
                  appearance.setHighContrast(!appearance.highContrast)
                }}
              >
                <span class={css({ display: "grid", gap: "4px", textAlign: "left" })}>
                  <span
                    class={css({
                      color: "text",
                      fontSize: "0.94rem",
                      fontWeight: "700",
                    })}
                  >
                    {appearance.highContrast ? "Enabled" : "Disabled"}
                  </span>
                  <span
                    class={css({
                      color: "muted",
                      fontSize: "0.82rem",
                      lineHeight: "1.65",
                    })}
                  >
                    Keep the palette compact while making the interface read a little sharper.
                  </span>
                </span>
                <span
                  aria-hidden="true"
                  class={css({
                    position: "relative",
                    display: "inline-flex",
                    alignItems: "center",
                    width: "44px",
                    height: "26px",
                    padding: "3px",
                    borderRadius: "999px",
                    backgroundColor: "panel",
                    transition: "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                    "&[data-enabled='true']": {
                      backgroundColor: "accent",
                    },
                  })}
                  data-enabled={appearance.highContrast}
                >
                  <span
                    class={css({
                      width: "20px",
                      height: "20px",
                      borderRadius: "999px",
                      backgroundColor: "background",
                      transition: "transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
                      "&[data-enabled='true']": {
                        transform: "translateX(18px)",
                      },
                    })}
                    data-enabled={appearance.highContrast}
                  />
                </span>
              </button>
            </section>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
