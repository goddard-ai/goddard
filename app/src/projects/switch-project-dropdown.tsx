import { Popover } from "@ark-ui/react/popover"
import { css } from "@goddard-ai/styled-system/css"
import { Search } from "lucide-react"
import { useEffect, useRef, useState } from "preact/hooks"

import type { ProjectRecord } from "./project-registry.ts"

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
  activeProjectLabel: string
  activeProjectPath: string | null
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  onOpenFolder: () => Promise<void> | void
  onSelectProject: (path: string) => void
  projects: readonly ProjectRecord[]
}) {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const itemRefs = useRef<Record<string, HTMLButtonElement | null>>({})
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
    }
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
    <Popover.Root
      closeOnInteractOutside={true}
      initialFocusEl={() => inputRef.current}
      lazyMount={true}
      open={props.isOpen}
      portalled={false}
      positioning={{ placement: "bottom", sameWidth: true }}
      unmountOnExit={true}
      onOpenChange={(details) => {
        props.onOpenChange(details.open)
      }}
    >
      <Popover.Trigger asChild>
        <button
          aria-label="Switch project"
          class={css({
            display: "inline-flex",
            alignItems: "center",
            width: "100%",
            height: "28px",
            paddingInline: "12px",
            border: "none",
            borderRadius: "6px",
            backgroundColor: "panel",
            color: "muted",
            cursor: "pointer",
            textAlign: "left",
            transition:
              "background-color 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
            _hover: {
              backgroundColor: "surface",
              color: "text",
            },
            _focusVisible: {
              outline: "2px solid",
              outlineColor: "accentStrong",
              outlineOffset: "2px",
            },
          })}
          type="button"
        >
          <span
            class={css({
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontSize: "13px",
              fontWeight: "400",
              letterSpacing: "0.02em",
              lineHeight: "1.21",
            })}
          >
            {props.activeProjectLabel}
          </span>
        </button>
      </Popover.Trigger>

      <Popover.Positioner>
        <Popover.Content
          class={css({
            display: "grid",
            gap: "8px",
            width: "var(--reference-width)",
            minWidth: "320px",
            marginTop: "10px",
            padding: "10px",
            border: "1px solid",
            borderColor: "border",
            borderRadius: "14px",
            backgroundColor: "background",
            outline: "none",
          })}
        >
          <label
            class={css({
              display: "flex",
              alignItems: "center",
              gap: "10px",
              minHeight: "38px",
              paddingInline: "12px",
              borderRadius: "10px",
              border: "1px solid",
              borderColor: "border",
              backgroundColor: "surface",
              color: "muted",
              transition:
                "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
              _focusWithin: {
                borderColor: "accent",
                backgroundColor: "surface",
              },
            })}
          >
            <span
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
              Search projects
            </span>
            <Search aria-hidden={true} size={16} strokeWidth={2.1} />
            <input
              ref={inputRef}
              class={css({
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
              })}
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
              }}
            />
          </label>

          {items.length > 0 ? (
            <ul
              class={css({
                display: "grid",
                gap: "2px",
                maxHeight: "280px",
                overflowY: "auto",
                listStyle: "none",
                padding: "0",
                margin: "0",
              })}
            >
              {items.map((item, index) => {
                const isHighlighted = highlightedIndex === index

                if (item.kind === "open-folder") {
                  return (
                    <li key={item.id}>
                      <button
                        ref={(element) => {
                          itemRefs.current[item.id] = element
                        }}
                        class={css({
                          display: "flex",
                          alignItems: "flex-start",
                          justifyContent: "space-between",
                          gap: "12px",
                          width: "100%",
                          minHeight: "44px",
                          padding: "8px 10px",
                          border: "none",
                          borderRadius: "10px",
                          backgroundColor: "transparent",
                          color: "text",
                          cursor: "pointer",
                          textAlign: "left",
                          transition: "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                          _focusVisible: {
                            outline: "2px solid",
                            outlineColor: "accentStrong",
                            outlineOffset: "2px",
                          },
                          "&[data-highlighted='true']": {
                            backgroundColor: "surface",
                          },
                        })}
                        data-highlighted={isHighlighted ? "true" : "false"}
                        type="button"
                        onMouseEnter={() => {
                          setHighlightedIndex(index)
                        }}
                        onClick={() => {
                          activateItem(item)
                        }}
                      >
                        <span class={css({ display: "grid", gap: "2px", minWidth: "0" })}>
                          <span
                            class={css({
                              fontSize: "0.88rem",
                              fontWeight: "650",
                            })}
                          >
                            Open folder…
                          </span>
                          <span
                            class={css({
                              color: "muted",
                              fontSize: "0.76rem",
                              lineHeight: "1.4",
                            })}
                          >
                            Add a local project from your filesystem.
                          </span>
                        </span>
                      </button>
                    </li>
                  )
                }

                const isActiveProject = props.activeProjectPath === item.project.path

                return (
                  <li key={item.id}>
                    <button
                      ref={(element) => {
                        itemRefs.current[item.id] = element
                      }}
                      class={css({
                        display: "flex",
                        alignItems: "flex-start",
                        justifyContent: "space-between",
                        gap: "12px",
                        width: "100%",
                        minHeight: "44px",
                        padding: "8px 10px",
                        border: "none",
                        borderRadius: "10px",
                        backgroundColor: "transparent",
                        color: "text",
                        cursor: "pointer",
                        textAlign: "left",
                        transition: "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
                        _focusVisible: {
                          outline: "2px solid",
                          outlineColor: "accentStrong",
                          outlineOffset: "2px",
                        },
                        "&[data-highlighted='true']": {
                          backgroundColor: "surface",
                        },
                      })}
                      data-highlighted={isHighlighted ? "true" : "false"}
                      type="button"
                      onMouseEnter={() => {
                        setHighlightedIndex(index)
                      }}
                      onClick={() => {
                        activateItem(item)
                      }}
                    >
                      <span class={css({ display: "grid", gap: "2px", minWidth: "0" })}>
                        <span
                          class={css({
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                            fontSize: "0.88rem",
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
                            lineHeight: "1.4",
                          })}
                        >
                          {item.project.path}
                        </span>
                      </span>

                      {isActiveProject ? (
                        <span
                          class={css({
                            color: "accentStrong",
                            fontSize: "0.72rem",
                            fontWeight: "600",
                            whiteSpace: "nowrap",
                          })}
                        >
                          Active
                        </span>
                      ) : null}
                    </button>
                  </li>
                )
              })}
            </ul>
          ) : (
            <p
              class={css({
                margin: "0",
                padding: "10px",
                color: "muted",
                fontSize: "0.84rem",
              })}
            >
              No matching projects.
            </p>
          )}
        </Popover.Content>
      </Popover.Positioner>
    </Popover.Root>
  )
}

export default SwitchProjectDropdown
