import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useListener } from "preact-sigma"
import { FolderOpen, Search } from "lucide-react"
import { useEffect, useRef, useState } from "preact/hooks"

import type { ProjectRecord } from "./project-registry.ts"

const dropdownClass = css({
  position: "absolute",
  top: "calc(100% + 10px)",
  left: "0",
  display: "grid",
  gap: "8px",
  width: "100%",
  minWidth: "320px",
  padding: "10px",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "18px",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  boxShadow: `0 24px 56px ${token.var("colors.shadow")}`,
  zIndex: "10",
})

const inputShellClass = css({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  minHeight: "42px",
  paddingInline: "12px",
  borderRadius: "12px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "muted",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusWithin: {
    borderColor: "accent",
    backgroundColor: "surface",
  },
})

const inputClass = css({
  width: "100%",
  height: "28px",
  border: "none",
  outline: "none",
  backgroundColor: "transparent",
  color: "text",
  fontSize: "0.9rem",
  fontWeight: "600",
  "&::placeholder": {
    color: "muted",
  },
})

const listClass = css({
  display: "grid",
  gap: "4px",
  maxHeight: "320px",
  overflowY: "auto",
})

const itemClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  alignItems: "center",
  gap: "12px",
  width: "100%",
  minHeight: "52px",
  padding: "10px 12px",
  border: "1px solid",
  borderColor: "transparent",
  borderRadius: "14px",
  backgroundColor: "transparent",
  color: "text",
  cursor: "pointer",
  textAlign: "left",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    outline: "2px solid",
    outlineColor: "accentStrong",
    outlineOffset: "2px",
  },
  "&[data-highlighted='true']": {
    backgroundColor: "surface",
    borderColor: "accent",
    boxShadow: `0 12px 28px ${token.var("colors.shadow")}`,
  },
})

type SwitchProjectItem =
  | {
      id: "open-folder"
      kind: "open-folder"
    }
  | {
      id: string
      kind: "project"
      project: ProjectRecord
    }

function projectMatchesSearch(project: ProjectRecord, search: string) {
  const normalizedSearch = search.trim().toLowerCase()

  if (normalizedSearch.length === 0) {
    return true
  }

  return (
    project.name.toLowerCase().includes(normalizedSearch) ||
    project.path.toLowerCase().includes(normalizedSearch)
  )
}

/** Renders the header-anchored searchable project switcher dropdown. */
export function SwitchProjectDropdown(props: {
  activeProjectPath: string | null
  containerRef: { current: HTMLDivElement | null }
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  onOpenFolder: () => Promise<void> | void
  onSelectProject: (path: string) => void
  projects: readonly ProjectRecord[]
}) {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})
  const rootRef = useRef<HTMLDivElement | null>(null)
  const [search, setSearch] = useState("")
  const [highlightedIndex, setHighlightedIndex] = useState(0)

  const filteredProjects = props.projects.filter((project) => projectMatchesSearch(project, search))
  const items: SwitchProjectItem[] =
    search.trim().length === 0
      ? [{ id: "open-folder", kind: "open-folder" }, ...filteredProjects.map((project) => ({
          id: project.path,
          kind: "project" as const,
          project,
        }))]
      : filteredProjects.map((project) => ({
          id: project.path,
          kind: "project" as const,
          project,
        }))
  const highlightedItemId = items[highlightedIndex]?.id ?? null

  useEffect(() => {
    if (!props.isOpen) {
      setSearch("")
      setHighlightedIndex(0)
      return
    }

    setSearch("")
    setHighlightedIndex(0)

    queueMicrotask(() => {
      inputRef.current?.focus()
      inputRef.current?.select()
    })
  }, [props.isOpen])

  useEffect(() => {
    setHighlightedIndex((currentIndex) => {
      if (items.length === 0) {
        return 0
      }

      return Math.min(currentIndex, items.length - 1)
    })
  }, [items.length])

  useEffect(() => {
    if (!props.isOpen || !highlightedItemId) {
      return
    }

    itemRefs.current[highlightedItemId]?.scrollIntoView({
      block: "nearest",
    })
  }, [highlightedItemId, props.isOpen])

  useListener(document, "mousedown", (event) => {
    if (!props.isOpen) {
      return
    }

    const target = event.target

    if (!(target instanceof Node)) {
      return
    }

    if (rootRef.current?.contains(target) || props.containerRef.current?.contains(target)) {
      return
    }

    props.onOpenChange(false)
  })

  if (!props.isOpen) {
    return null
  }

  function activateItem(item: SwitchProjectItem | undefined) {
    if (!item) {
      return
    }

    props.onOpenChange(false)

    if (item.kind === "open-folder") {
      void props.onOpenFolder()
      return
    }

    props.onSelectProject(item.project.path)
  }

  return (
    <div ref={rootRef} class={dropdownClass}>
      <div class={inputShellClass}>
        <Search aria-hidden={true} size={16} strokeWidth={2.1} />
        <input
          ref={inputRef}
          class={inputClass}
          placeholder="Search projects"
          value={search}
          onInput={(event) => {
            setSearch(event.currentTarget.value)
            setHighlightedIndex(0)
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault()
              setHighlightedIndex((currentIndex) =>
                items.length === 0 ? 0 : Math.min(currentIndex + 1, items.length - 1),
              )
              return
            }

            if (event.key === "ArrowUp") {
              event.preventDefault()
              setHighlightedIndex((currentIndex) => Math.max(currentIndex - 1, 0))
              return
            }

            if (event.key === "Enter") {
              event.preventDefault()
              activateItem(items[highlightedIndex])
              return
            }

            if (event.key === "Escape") {
              event.preventDefault()
              props.onOpenChange(false)
            }
          }}
        />
      </div>

      {items.length > 0 ? (
        <div class={listClass}>
          {items.map((item, index) => {
            const isHighlighted = highlightedIndex === index

            if (item.kind === "open-folder") {
              return (
                <button
                  key={item.id}
                  ref={(element) => {
                    itemRefs.current[item.id] = element
                  }}
                  class={itemClass}
                  data-highlighted={isHighlighted ? "true" : "false"}
                  type="button"
                  onMouseEnter={() => {
                    setHighlightedIndex(index)
                  }}
                  onClick={() => {
                    activateItem(item)
                  }}
                >
                  <span class={css({ display: "grid", gap: "3px", minWidth: "0" })}>
                    <span
                      class={css({
                        display: "inline-flex",
                        alignItems: "center",
                        gap: "10px",
                        minWidth: "0",
                        fontSize: "0.9rem",
                        fontWeight: "650",
                      })}
                    >
                      <FolderOpen size={16} strokeWidth={2} />
                      <span>Open folder…</span>
                    </span>
                    <span
                      class={css({
                        color: "muted",
                        fontSize: "0.76rem",
                        lineHeight: "1.5",
                      })}
                    >
                      Add a local project from your filesystem.
                    </span>
                  </span>
                </button>
              )
            }

            const isActiveProject = props.activeProjectPath === item.project.path

            return (
              <button
                key={item.id}
                ref={(element) => {
                  itemRefs.current[item.id] = element
                }}
                class={itemClass}
                data-highlighted={isHighlighted ? "true" : "false"}
                type="button"
                onMouseEnter={() => {
                  setHighlightedIndex(index)
                }}
                onClick={() => {
                  activateItem(item)
                }}
              >
                <span class={css({ display: "grid", gap: "3px", minWidth: "0" })}>
                  <span
                    class={css({
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      fontSize: "0.9rem",
                      fontWeight: "650",
                    })}
                  >
                    {item.project.name}
                  </span>
                  <span
                    class={css({
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      color: "muted",
                      fontSize: "0.76rem",
                      lineHeight: "1.5",
                    })}
                  >
                    {item.project.path}
                  </span>
                </span>

                <span
                  class={cx(
                    css({
                      display: "inline-flex",
                      alignItems: "center",
                      justifyContent: "center",
                      minWidth: "42px",
                      height: "22px",
                      paddingInline: "8px",
                      borderRadius: "999px",
                      border: "1px solid",
                      borderColor: "border",
                      backgroundColor: "background",
                      color: "muted",
                      fontSize: "0.68rem",
                      fontWeight: "700",
                      letterSpacing: "0.04em",
                    }),
                    isActiveProject ? css({ color: "accentStrong", borderColor: "accent" }) : null,
                  )}
                >
                  {isActiveProject ? "Active" : "Project"}
                </span>
              </button>
            )
          })}
        </div>
      ) : (
        <div
          class={css({
            display: "grid",
            placeItems: "center",
            minHeight: "88px",
            color: "muted",
            fontSize: "0.84rem",
            fontWeight: "600",
          })}
        >
          No matching projects.
        </div>
      )}
    </div>
  )
}

export default SwitchProjectDropdown
