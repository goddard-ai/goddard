import { Dialog } from "@ark-ui/react/dialog"
import { Portal } from "@ark-ui/react/portal"
import { css, cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import { ArrowUpRight, FolderSearch2, Plus, Trash2, X } from "lucide-react"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { browseForProject as browseForProjectPath } from "~/desktop-host.ts"
import { deriveProjectName } from "./project-name.ts"
import { lookupProject, type ProjectRecord } from "./project-registry.ts"

const panelClass = css({
  display: "flex",
  flexDirection: "column",
  minHeight: "0",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "16px",
  backgroundColor: "panel",
  overflow: "hidden",
})

const titleClass = css({
  color: "text",
  fontSize: "1.25rem",
  fontWeight: "700",
  lineHeight: "1.25",
})

const bodyClass = css({
  color: "muted",
  fontSize: "0.9rem",
  lineHeight: "1.6",
})

const quietLabelClass = css({
  color: "muted",
  fontSize: "0.78rem",
  fontWeight: "600",
})

const labelClass = css({
  color: "text",
  fontSize: "0.84rem",
  fontWeight: "600",
})

const buttonBaseClass = css({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "8px",
  minWidth: "0",
  height: "36px",
  paddingInline: "12px",
  borderRadius: "10px",
  border: "1px solid",
  fontSize: "0.86rem",
  fontWeight: "600",
  cursor: "pointer",
  transition:
    "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.48",
  },
})

const secondaryButtonClass = css({
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  "@media (hover: hover) and (pointer: fine)": {
    _hover: {
      borderColor: "accent",
      backgroundColor: "surface",
    },
  },
})

const primaryButtonClass = css({
  borderColor: "accent",
  backgroundColor: "surface",
  color: "text",
  "@media (hover: hover) and (pointer: fine)": {
    _hover: {
      borderColor: "accentStrong",
      backgroundColor: "background",
    },
  },
})

const dangerButtonClass = css({
  borderColor: "border",
  backgroundColor: "background",
  color: "muted",
  "@media (hover: hover) and (pointer: fine)": {
    _hover: {
      borderColor: "danger",
      color: "danger",
      backgroundColor: "surface",
    },
  },
})

const dialogOverlayClass = css({
  position: "fixed",
  inset: "0",
  backgroundColor: "overlay",
  backdropFilter: "blur(6px)",
  opacity: "1",
  transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  "@starting-style": {
    opacity: "0",
  },
})

const dialogContentClass = css({
  width: "min(560px, calc(100vw - 32px))",
  maxHeight: "calc(100vh - 32px)",
  overflowY: "auto",
  padding: "20px",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "panel",
  opacity: "1",
  transform: "translateY(0)",
  transition:
    "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  outline: "none",
  "@starting-style": {
    opacity: "0",
    transform: "translateY(8px)",
  },
})

const dialogPositionerClass = css({
  position: "fixed",
  inset: "0",
  display: "grid",
  placeItems: "center",
  padding: "16px",
})

const textInputClass = css({
  width: "100%",
  height: "40px",
  paddingInline: "12px",
  borderRadius: "10px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  fontSize: "0.9rem",
  outline: "none",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    borderColor: "accentStrong",
  },
  _disabled: {
    opacity: "0.7",
  },
})

/** Renders the projects page plus its local add-project modal flow. */
export function ProjectsPage() {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const selectedProjectPath = useSignal<string | null>(projectRegistry.projectList[0]?.path ?? null)
  const isAddDialogOpen = useSignal(false)
  const draftPath = useSignal("")
  const draftName = useSignal("")
  const lastSuggestedName = useSignal<string | null>(null)
  const projects = projectRegistry.projectList
  const selectedProject = selectedProjectPath.value
    ? lookupProject(projectRegistry, selectedProjectPath.value)
    : null
  const derivedProjectName = deriveProjectName(draftPath.value)
  const canAddProject =
    draftPath.value.trim().length > 0 &&
    (draftName.value.trim().length > 0 || derivedProjectName.length > 0)

  useEffect(() => {
    if (!selectedProjectPath.value && projects[0]) {
      selectedProjectPath.value = projects[0].path
    }

    if (selectedProjectPath.value && !projectRegistry.projectsByPath[selectedProjectPath.value]) {
      selectedProjectPath.value = projects[0]?.path ?? null
    }
  }, [projectRegistry, projects, selectedProjectPath])

  function resetDraft(): void {
    draftPath.value = ""
    draftName.value = ""
    lastSuggestedName.value = null
  }

  function closeAddDialog(): void {
    isAddDialogOpen.value = false
    resetDraft()
  }

  async function browseForProject(): Promise<void> {
    const selectedPath = await browseForProjectPath()

    if (!selectedPath) {
      return
    }

    draftPath.value = selectedPath
    const suggestedName = deriveProjectName(selectedPath)

    if (draftName.value.length === 0 || draftName.value === lastSuggestedName.value) {
      draftName.value = suggestedName
    }

    lastSuggestedName.value = suggestedName
  }

  function addProject(): void {
    const projectPath = draftPath.value.trim()

    if (projectPath.length === 0) {
      return
    }

    const projectName = draftName.value.trim() || deriveProjectName(projectPath)

    if (projectName.length === 0) {
      return
    }

    projectRegistry.addProject({
      path: projectPath,
      name: projectName,
    })
    selectedProjectPath.value = projectPath
    closeAddDialog()
  }

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "minmax(0, 2fr) minmax(320px, 1fr)",
        gap: "20px",
        height: "100%",
        padding: "24px",
        "@media (max-width: 1040px)": {
          gridTemplateColumns: "1fr",
        },
      })}
    >
      <section class={panelClass}>
        <header
          class={css({
            display: "flex",
            alignItems: "flex-start",
            justifyContent: "space-between",
            gap: "16px",
            padding: "20px",
            borderBottom: "1px solid",
            borderColor: "border",
          })}
        >
          <div class={css({ display: "grid", gap: "6px", minWidth: "0" })}>
            <h1 class={titleClass}>Projects</h1>
            <p class={bodyClass}>Keep an explicit set of local roots available to the workbench.</p>
          </div>
          <button
            class={cx(buttonBaseClass, primaryButtonClass)}
            type="button"
            onClick={() => {
              resetDraft()
              isAddDialogOpen.value = true
            }}
          >
            <Plus size={16} strokeWidth={2.2} />
            Add project
          </button>
        </header>

        {projects.length === 0 ? (
          <div
            class={css({
              display: "grid",
              alignContent: "start",
              gap: "12px",
              minHeight: "220px",
              padding: "20px",
            })}
          >
            <div class={css({ display: "grid", gap: "8px", maxWidth: "30rem" })}>
              <h2 class={titleClass}>No projects yet</h2>
              <p class={bodyClass}>
                Add a local directory to establish the app&apos;s explicit working set.
              </p>
              <div>
                <button
                  class={cx(buttonBaseClass, primaryButtonClass)}
                  type="button"
                  onClick={() => {
                    resetDraft()
                    isAddDialogOpen.value = true
                  }}
                >
                  <Plus size={16} strokeWidth={2.2} />
                  Add project
                </button>
              </div>
            </div>
          </div>
        ) : (
          <ProjectList
            onOpenProjectTab={(projectPath) => {
              const project = lookupProject(projectRegistry, projectPath)

              if (!project) {
                return
              }

              workbenchTabSet.openOrFocusTab({
                id: `project:${encodeURIComponent(project.path)}`,
                kind: "project",
                title: project.name,
                payload: { projectPath: project.path },
                dirty: false,
              })
            }}
            onRemove={(projectPath) => {
              projectContext.removeProject(projectPath)
              projectRegistry.removeProject(projectPath)
            }}
            onSelect={(projectPath) => {
              selectedProjectPath.value = projectPath
            }}
            projects={projects}
            selectedProjectPath={selectedProjectPath.value}
          />
        )}
      </section>

      <aside class={panelClass}>
        <div
          class={css({
            display: "grid",
            gap: "16px",
            padding: "20px",
          })}
        >
          <div class={css({ display: "grid", gap: "6px" })}>
            <h2
              class={css({
                color: "text",
                fontSize: "1rem",
                fontWeight: "700",
              })}
            >
              Project details
            </h2>
            <p class={bodyClass}>
              {selectedProject
                ? "Selected project metadata for the current workspace."
                : "Select a project to inspect its configured path and display name."}
            </p>
          </div>
          {selectedProject ? (
            <>
              <h3
                class={css({
                  color: "text",
                  fontSize: "1.05rem",
                  fontWeight: "700",
                })}
              >
                {selectedProject.name}
              </h3>
              <div class={css({ display: "grid", gap: "12px" })}>
                <InfoCard label="Path" value={selectedProject.path} />
                <InfoCard label="Display name" value={selectedProject.name} />
              </div>
            </>
          ) : null}
        </div>
      </aside>

      <AddProjectDialog
        addProject={addProject}
        browseForProject={browseForProject}
        canAddProject={canAddProject}
        closeAddDialog={closeAddDialog}
        draftName={draftName.value}
        draftPath={draftPath.value}
        isAddDialogOpen={isAddDialogOpen.value}
        onDraftNameInput={(value) => {
          draftName.value = value
        }}
        onDraftPathInput={(value) => {
          draftPath.value = value
          const suggestedName = deriveProjectName(value)

          if (draftName.value.length === 0 || draftName.value === lastSuggestedName.value) {
            draftName.value = suggestedName
          }

          lastSuggestedName.value = suggestedName
        }}
      />
    </div>
  )
}

export default ProjectsPage

/** Renders the machine-wide project list. */
function ProjectList(props: {
  projects: readonly ProjectRecord[]
  selectedProjectPath: string | null
  onSelect: (id: string) => void
  onRemove: (id: string) => void
  onOpenProjectTab: (id: string) => void
}) {
  return (
    <ul
      class={css({
        listStyle: "none",
        margin: "0",
        padding: "0",
        minHeight: "0",
        overflowY: "auto",
        "& > li + li": {
          borderTop: "1px solid",
          borderColor: "border",
        },
      })}
    >
      {props.projects.map((project) => (
        <li key={project.path}>
          <ProjectListRow
            isSelected={props.selectedProjectPath === project.path}
            onOpenProjectTab={props.onOpenProjectTab}
            onRemove={props.onRemove}
            onSelect={props.onSelect}
            project={project}
          />
        </li>
      ))}
    </ul>
  )
}

/** Renders one project row with quick actions and identity metadata. */
function ProjectListRow(props: {
  project: ProjectRecord
  isSelected: boolean
  onSelect: (id: string) => void
  onRemove: (id: string) => void
  onOpenProjectTab: (id: string) => void
}) {
  return (
    <article
      class={css({
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) auto",
        gap: "12px",
        padding: "14px 16px",
        borderInlineStart: "2px solid",
        borderInlineStartColor: props.isSelected ? "accent" : "transparent",
        backgroundColor: props.isSelected ? "surface" : "transparent",
      })}
    >
      <button
        class={css({
          display: "grid",
          gap: "4px",
          minWidth: "0",
          padding: "0",
          border: "none",
          background: "transparent",
          color: "inherit",
          textAlign: "left",
          cursor: "pointer",
        })}
        type="button"
        onClick={() => {
          props.onSelect(props.project.path)
        }}
      >
        <div
          class={css({
            display: "flex",
            flexWrap: "wrap",
            alignItems: "center",
            gap: "8px",
          })}
        >
          <h3
            class={css({
              color: "text",
              fontSize: "0.96rem",
              fontWeight: "600",
              lineHeight: "1.4",
            })}
          >
            {props.project.name}
          </h3>
          {props.isSelected ? (
            <span
              class={css({
                color: "accentStrong",
                fontSize: "0.78rem",
                fontWeight: "600",
              })}
            >
              Selected
            </span>
          ) : null}
        </div>
        <p
          class={css({
            color: "muted",
            fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
            fontSize: "0.8rem",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          })}
        >
          {props.project.path}
        </p>
      </button>

      <div
        class={css({
          display: "flex",
          alignItems: "center",
          gap: "8px",
        })}
      >
        <button
          class={cx(buttonBaseClass, secondaryButtonClass)}
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            props.onOpenProjectTab(props.project.path)
          }}
        >
          <ArrowUpRight size={15} strokeWidth={2.2} />
          Open tab
        </button>
        <button
          class={cx(buttonBaseClass, dangerButtonClass)}
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            props.onRemove(props.project.path)
          }}
        >
          <Trash2 size={15} strokeWidth={2.1} />
          Remove
        </button>
      </div>
    </article>
  )
}

/** Renders one small inspector card. */
function InfoCard(props: { label: string; value: string }) {
  return (
    <dl
      class={css({
        display: "grid",
        gap: "4px",
        padding: "12px",
        borderRadius: "12px",
        border: "1px solid",
        borderColor: "border",
      })}
    >
      <dt class={quietLabelClass}>{props.label}</dt>
      <dd
        class={css({
          margin: "0",
          color: "text",
          fontWeight: "600",
          lineHeight: "1.6",
          wordBreak: "break-word",
        })}
      >
        {props.value}
      </dd>
    </dl>
  )
}

/** Renders the modal flow used to add one project. */
function AddProjectDialog(props: {
  isAddDialogOpen: boolean
  draftPath: string
  draftName: string
  canAddProject: boolean
  closeAddDialog: () => void
  browseForProject: () => Promise<void>
  onDraftPathInput: (value: string) => void
  onDraftNameInput: (value: string) => void
  addProject: () => void
}) {
  return (
    <Dialog.Root
      open={props.isAddDialogOpen}
      onOpenChange={(details: { open: boolean }) => {
        if (!details.open) {
          props.closeAddDialog()
        }
      }}
    >
      <Portal>
        <Dialog.Backdrop class={dialogOverlayClass} />
        <Dialog.Positioner class={dialogPositionerClass}>
          <Dialog.Content class={dialogContentClass}>
            <form
              class={css({
                display: "grid",
                gap: "16px",
              })}
              onSubmit={(event) => {
                event.preventDefault()
                props.addProject()
              }}
            >
              <header
                class={css({
                  display: "flex",
                  alignItems: "flex-start",
                  justifyContent: "space-between",
                  gap: "16px",
                })}
              >
                <div class={css({ display: "grid", gap: "6px" })}>
                  <Dialog.Title class={titleClass}>Add project</Dialog.Title>
                  <Dialog.Description class={bodyClass}>
                    Choose one local root and optionally adjust the display name.
                  </Dialog.Description>
                </div>
                <Dialog.CloseTrigger asChild>
                  <button
                    class={css({
                      display: "grid",
                      placeItems: "center",
                      width: "32px",
                      height: "32px",
                      borderRadius: "10px",
                      border: "1px solid",
                      borderColor: "border",
                      backgroundColor: "background",
                      color: "muted",
                      cursor: "pointer",
                    })}
                    type="button"
                  >
                    <X size={15} strokeWidth={2.2} />
                  </button>
                </Dialog.CloseTrigger>
              </header>

              <label
                class={css({
                  display: "grid",
                  gap: "6px",
                })}
              >
                <span class={labelClass}>Project path</span>
                <input
                  class={textInputClass}
                  placeholder="/Users/alec/dev/goddard-ai/app"
                  type="text"
                  value={props.draftPath}
                  onInput={(event) => {
                    props.onDraftPathInput(event.currentTarget.value)
                  }}
                />
              </label>

              <div
                class={css({
                  display: "flex",
                  justifyContent: "flex-start",
                })}
              >
                <button
                  class={cx(buttonBaseClass, secondaryButtonClass)}
                  type="button"
                  onClick={() => {
                    void props.browseForProject()
                  }}
                >
                  <FolderSearch2 size={15} strokeWidth={2.1} />
                  Browse
                </button>
              </div>

              <label
                class={css({
                  display: "grid",
                  gap: "6px",
                })}
              >
                <span class={labelClass}>Display name</span>
                <input
                  class={textInputClass}
                  placeholder="Derived from folder name"
                  type="text"
                  value={props.draftName}
                  onInput={(event) => {
                    props.onDraftNameInput(event.currentTarget.value)
                  }}
                />
              </label>

              <div
                class={css({
                  display: "flex",
                  justifyContent: "flex-end",
                  gap: "8px",
                })}
              >
                <Dialog.CloseTrigger asChild>
                  <button class={cx(buttonBaseClass, secondaryButtonClass)} type="button">
                    Cancel
                  </button>
                </Dialog.CloseTrigger>
                <button
                  class={cx(buttonBaseClass, primaryButtonClass)}
                  disabled={!props.canAddProject}
                  type="button"
                  onClick={() => {
                    props.addProject()
                  }}
                >
                  <Plus size={15} strokeWidth={2.2} />
                  Add project
                </button>
              </div>
            </form>
          </Dialog.Content>
        </Dialog.Positioner>
      </Portal>
    </Dialog.Root>
  )
}
