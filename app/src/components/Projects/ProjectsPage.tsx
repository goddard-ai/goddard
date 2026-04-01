import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import * as Dialog from "@radix-ui/react-dialog"
import { useMutation } from "@tanstack/preact-query"
import {
  AlertCircle,
  ArrowUpRight,
  BadgeCheck,
  CheckCircle2,
  Folder,
  FolderKanban,
  FolderSearch2,
  PencilLine,
  Plus,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react"
import type { ComponentChild } from "preact"
import { useEffect } from "preact/hooks"
import { pickProjectPath, validateProjectPath } from "../../support/project-service"
import { useProjectRegistry, useWorkbenchTabSet } from "../state/AppStateContext"
import { lookupProject, type ProjectRecord } from "./state/ProjectRegistry"

const panelClass = css({
  display: "flex",
  flexDirection: "column",
  minHeight: "0",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "28px",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 28px 80px rgba(121, 138, 160, 0.12)",
})

const eyebrowClass = css({
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
  letterSpacing: "0.16em",
  textTransform: "uppercase",
  boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
})

const titleClass = css({
  color: "text",
  fontSize: "1.55rem",
  fontWeight: "760",
  letterSpacing: "-0.03em",
  lineHeight: "1.1",
})

const bodyClass = css({
  color: "muted",
  fontSize: "0.95rem",
  lineHeight: "1.7",
})

const quietLabelClass = css({
  color: "muted",
  fontSize: "0.78rem",
  fontWeight: "700",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

const buttonBaseClass = css({
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "8px",
  minWidth: "0",
  height: "40px",
  paddingInline: "14px",
  borderRadius: "14px",
  border: "1px solid",
  fontSize: "0.88rem",
  fontWeight: "640",
  cursor: "pointer",
  transition:
    "transform 160ms cubic-bezier(0.23, 1, 0.32, 1), background-color 180ms cubic-bezier(0.23, 1, 0.32, 1), border-color 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  _active: {
    transform: "scale(0.97)",
  },
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.48",
    boxShadow: "none",
    transform: "none",
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
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  color: "text",
  boxShadow: `0 12px 28px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent)`,
  "@media (hover: hover) and (pointer: fine)": {
    _hover: {
      borderColor: "accentStrong",
      boxShadow: `0 18px 34px color-mix(in srgb, ${token.var("colors.accent")} 18%, transparent)`,
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

const iconBadgeClass = css({
  display: "grid",
  placeItems: "center",
  width: "42px",
  height: "42px",
  borderRadius: "16px",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  color: "accentStrong",
  boxShadow: `0 14px 28px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
  flexShrink: "0",
})

const rowClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  gap: "20px",
  padding: "18px",
  borderRadius: "22px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
  transition:
    "transform 180ms cubic-bezier(0.23, 1, 0.32, 1), border-color 180ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1), background-color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  cursor: "pointer",
  outline: "none",
  _active: {
    transform: "scale(0.992)",
  },
  _focusVisible: {
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 18%, transparent)`,
  },
  "@media (hover: hover) and (pointer: fine)": {
    _hover: {
      transform: "translateY(-1px)",
      borderColor: "accent",
      boxShadow: `0 18px 32px color-mix(in srgb, ${token.var("colors.accent")} 10%, transparent)`,
    },
  },
  "&[data-selected='true']": {
    borderColor: "accentStrong",
    boxShadow: `0 18px 40px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), inset 0 0 0 1px color-mix(in srgb, ${token.var("colors.accentStrong")} 18%, transparent)`,
  },
})

const dialogOverlayClass = css({
  position: "fixed",
  inset: "0",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 10%, rgba(23, 29, 36, 0.24)), rgba(23, 29, 36, 0.34))`,
  backdropFilter: "blur(10px)",
  opacity: "1",
  transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
  "@starting-style": {
    opacity: "0",
  },
})

const dialogContentClass = css({
  position: "fixed",
  top: "50%",
  left: "50%",
  width: "min(620px, calc(100vw - 32px))",
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
})

const textInputClass = css({
  width: "100%",
  height: "48px",
  paddingInline: "16px",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  fontSize: "0.95rem",
  outline: "none",
  transition:
    "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1), background-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  _focusVisible: {
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
  },
  _disabled: {
    opacity: "0.7",
  },
})

/** Renders the projects page plus its local add-project modal flow. */
export function ProjectsPage() {
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

  const projectValidation = useMutation({
    mutationFn: async (path: string) => await validateProjectPath(path),
    onSuccess: (project) => {
      draftPath.value = project.path

      if (draftName.value.length === 0 || draftName.value === lastSuggestedName.value) {
        draftName.value = project.name
      }

      lastSuggestedName.value = project.name
    },
  })

  const validatedProject =
    projectValidation.data && projectValidation.data.path === draftPath.value.trim()
      ? projectValidation.data
      : null
  const mutationError: unknown = projectValidation.error
  const validationError =
    mutationError instanceof Error
      ? mutationError.message
      : mutationError
        ? String(mutationError)
        : null
  const canAddProject =
    !projectValidation.isPending &&
    validatedProject !== null &&
    (draftName.value.trim().length > 0 || validatedProject.name.length > 0)

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
    projectValidation.reset()
  }

  function openAddDialog(): void {
    resetDraft()
    isAddDialogOpen.value = true
  }

  function closeAddDialog(): void {
    isAddDialogOpen.value = false
    resetDraft()
  }

  function clearSuggestedName(): void {
    if (draftName.value === lastSuggestedName.value) {
      draftName.value = ""
    }

    lastSuggestedName.value = null
  }

  async function inspectDraftPath(rawPath: string): Promise<void> {
    const trimmedPath = rawPath.trim()

    if (trimmedPath.length === 0) {
      projectValidation.reset()
      clearSuggestedName()
      return
    }

    try {
      await projectValidation.mutateAsync(trimmedPath)
    } catch {}
  }

  async function browseForProject(): Promise<void> {
    const selectedPath = await pickProjectPath()

    if (!selectedPath) {
      return
    }

    draftPath.value = selectedPath
    await inspectDraftPath(selectedPath)
  }

  function openProjectTab(projectPath: string): void {
    const project = lookupProject(projectRegistry, projectPath)

    if (!project) {
      return
    }

    workbenchTabSet.openOrFocusTab({
      id: `project:${encodeURIComponent(project.path)}`,
      kind: "project",
      title: project.name,
      icon: "projects",
      payload: { projectPath: project.path },
      dirty: false,
    })
  }

  function addProject(): void {
    if (!validatedProject) {
      return
    }

    const projectName = draftName.value.trim() || validatedProject.name
    projectRegistry.addProject({
      path: validatedProject.path,
      name: projectName,
    })
    selectedProjectPath.value = validatedProject.path
    closeAddDialog()
  }

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "minmax(0, 2fr) minmax(320px, 1fr)",
        gap: "22px",
        height: "100%",
        padding: "26px",
        background:
          `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 36%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
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
            padding: "26px 28px 20px",
            borderBottom: "1px solid",
            borderColor: "border",
          })}
        >
          <div
            class={css({ display: "flex", flexDirection: "column", gap: "12px", minWidth: "0" })}
          >
            <span class={eyebrowClass}>
              <Sparkles size={14} strokeWidth={2} />
              Machine-wide scope
            </span>
            <div class={css({ display: "flex", flexDirection: "column", gap: "8px" })}>
              <h1 class={titleClass}>Projects</h1>
              <p class={bodyClass}>
                Keep an explicit set of local roots for sessions, specs, tasks, pull requests, and
                the rest of the workbench.
              </p>
            </div>
          </div>
          <button
            class={cx(buttonBaseClass, primaryButtonClass)}
            type="button"
            onClick={() => {
              openAddDialog()
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
              placeItems: "center",
              minHeight: "320px",
              padding: "40px",
            })}
          >
            <div
              class={css({
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: "16px",
                maxWidth: "34rem",
                textAlign: "center",
              })}
            >
              <div class={iconBadgeClass}>
                <FolderKanban size={20} strokeWidth={1.9} />
              </div>
              <h2 class={titleClass}>No projects yet</h2>
              <p class={bodyClass}>
                Add a local directory to establish the app&apos;s explicit working set. Nothing is
                inferred until you choose it.
              </p>
            </div>
          </div>
        ) : (
          <ProjectList
            onOpenProjectTab={openProjectTab}
            onRemove={(projectPath) => {
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
            display: "flex",
            flexDirection: "column",
            gap: "18px",
            padding: "26px",
          })}
        >
          <span class={eyebrowClass}>
            <PencilLine size={14} strokeWidth={2} />
            Inspector
          </span>
          {selectedProject ? (
            <>
              <div class={css({ display: "flex", alignItems: "center", gap: "14px" })}>
                <div class={iconBadgeClass}>
                  <Folder size={18} strokeWidth={1.95} />
                </div>
                <div
                  class={css({
                    display: "flex",
                    flexDirection: "column",
                    gap: "6px",
                    minWidth: "0",
                  })}
                >
                  <h2 class={titleClass}>{selectedProject.name}</h2>
                  <p class={bodyClass}>Selected root for the current workspace scope.</p>
                </div>
              </div>

              <div
                class={css({
                  display: "grid",
                  gap: "12px",
                })}
              >
                <InfoCard
                  icon={<Folder size={16} strokeWidth={1.95} />}
                  label="Path"
                  value={selectedProject.path}
                />
                <InfoCard
                  icon={<BadgeCheck size={16} strokeWidth={1.95} />}
                  label="Display name"
                  value={selectedProject.name}
                />
              </div>

              <p class={bodyClass}>
                Projects stay intentionally lightweight here: one local path plus one display name.
                Git-specific checks and deeper capability discovery can be layered in where a real
                screen actually needs them.
              </p>
            </>
          ) : (
            <>
              <div class={iconBadgeClass}>
                <FolderSearch2 size={20} strokeWidth={1.9} />
              </div>
              <h2 class={titleClass}>No project selected</h2>
              <p class={bodyClass}>
                Pick a row from the list to inspect its path and configured display name.
              </p>
            </>
          )}
        </div>
      </aside>

      <AddProjectDialog
        addProject={addProject}
        browseForProject={browseForProject}
        canAddProject={canAddProject}
        closeAddDialog={closeAddDialog}
        draftName={draftName.value}
        draftPath={draftPath.value}
        inspectDraftPath={inspectDraftPath}
        isAddDialogOpen={isAddDialogOpen.value}
        isValidating={projectValidation.isPending}
        onDraftNameInput={(value) => {
          draftName.value = value
        }}
        onDraftPathInput={(value) => {
          draftPath.value = value
          projectValidation.reset()
          clearSuggestedName()
        }}
        validatedProject={validatedProject}
        validationError={validationError}
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
    <div
      class={css({
        display: "flex",
        flexDirection: "column",
        gap: "12px",
        minHeight: "0",
        padding: "14px",
        overflowY: "auto",
      })}
    >
      {props.projects.map((project) => (
        <ProjectListRow
          key={project.path}
          isSelected={props.selectedProjectPath === project.path}
          onOpenProjectTab={props.onOpenProjectTab}
          onRemove={props.onRemove}
          onSelect={props.onSelect}
          project={project}
        />
      ))}
    </div>
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
    <div
      class={rowClass}
      data-selected={props.isSelected}
      role="button"
      tabIndex={0}
      onClick={() => {
        props.onSelect(props.project.path)
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault()
          props.onSelect(props.project.path)
        }
      }}
    >
      <div
        class={css({
          display: "flex",
          alignItems: "flex-start",
          gap: "14px",
          minWidth: "0",
        })}
      >
        <div class={iconBadgeClass}>
          <Folder size={18} strokeWidth={1.95} />
        </div>
        <div
          class={css({
            display: "flex",
            flexDirection: "column",
            gap: "10px",
            minWidth: "0",
          })}
        >
          <div
            class={css({
              display: "flex",
              flexWrap: "wrap",
              alignItems: "center",
              gap: "10px",
            })}
          >
            <h3
              class={css({
                color: "text",
                fontSize: "1rem",
                fontWeight: "720",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              })}
            >
              {props.project.name}
            </h3>
            {props.isSelected ? (
              <span
                class={css({
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "6px 10px",
                  borderRadius: "999px",
                  backgroundColor: "surface",
                  color: "accentStrong",
                  fontSize: "0.76rem",
                  fontWeight: "700",
                  boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
                })}
              >
                <CheckCircle2 size={13} strokeWidth={2.3} />
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
        </div>
      </div>

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
    </div>
  )
}

/** Renders one small inspector card. */
function InfoCard(props: { icon: ComponentChild; label: string; value: string }) {
  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "40px minmax(0, 1fr)",
        gap: "12px",
        padding: "14px",
        borderRadius: "20px",
        border: "1px solid",
        borderColor: "border",
        backgroundColor: "surface",
      })}
    >
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          width: "40px",
          height: "40px",
          borderRadius: "14px",
          backgroundColor: "background",
          color: "accentStrong",
          boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
        })}
      >
        {props.icon}
      </div>
      <div class={css({ display: "flex", flexDirection: "column", gap: "6px", minWidth: "0" })}>
        <span class={quietLabelClass}>{props.label}</span>
        <span
          class={css({
            color: "text",
            fontWeight: "630",
            lineHeight: "1.6",
            wordBreak: "break-word",
          })}
        >
          {props.value}
        </span>
      </div>
    </div>
  )
}

/** Renders the modal flow used to add one project. */
function AddProjectDialog(props: {
  isAddDialogOpen: boolean
  draftPath: string
  draftName: string
  isValidating: boolean
  validatedProject: { path: string; name: string } | null
  validationError: string | null
  canAddProject: boolean
  closeAddDialog: () => void
  browseForProject: () => Promise<void>
  inspectDraftPath: (rawPath: string) => Promise<void>
  onDraftPathInput: (value: string) => void
  onDraftNameInput: (value: string) => void
  addProject: () => void
}) {
  return (
    <Dialog.Root
      open={props.isAddDialogOpen}
      onOpenChange={(open) => {
        if (!open) {
          props.closeAddDialog()
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay class={dialogOverlayClass} />
        <Dialog.Content class={dialogContentClass}>
          <div
            class={css({
              display: "flex",
              flexDirection: "column",
              gap: "22px",
            })}
          >
            <div
              class={css({
                display: "flex",
                alignItems: "flex-start",
                justifyContent: "space-between",
                gap: "16px",
              })}
            >
              <div class={css({ display: "flex", flexDirection: "column", gap: "12px" })}>
                <span class={eyebrowClass}>
                  <Plus size={14} strokeWidth={2.3} />
                  Add project
                </span>
                <div class={css({ display: "flex", flexDirection: "column", gap: "8px" })}>
                  <Dialog.Title class={titleClass}>Choose one local root</Dialog.Title>
                  <Dialog.Description class={bodyClass}>
                    Validation stays local to this dialog. The shared project registry only stores
                    the path and display name.
                  </Dialog.Description>
                </div>
              </div>
              <Dialog.Close asChild>
                <button class={cx(buttonBaseClass, secondaryButtonClass)} type="button">
                  <X size={15} strokeWidth={2.2} />
                  Close
                </button>
              </Dialog.Close>
            </div>

            <label
              class={css({
                display: "flex",
                flexDirection: "column",
                gap: "10px",
                color: "text",
                fontSize: "0.92rem",
                fontWeight: "650",
              })}
            >
              Project path
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
                flexWrap: "wrap",
                gap: "10px",
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
              <button
                class={cx(buttonBaseClass, secondaryButtonClass)}
                disabled={props.draftPath.trim().length === 0 || props.isValidating}
                type="button"
                onClick={() => {
                  void props.inspectDraftPath(props.draftPath)
                }}
              >
                <Search size={15} strokeWidth={2.1} />
                {props.isValidating ? "Validating..." : "Validate"}
              </button>
            </div>

            <label
              class={css({
                display: "flex",
                flexDirection: "column",
                gap: "10px",
                color: "text",
                fontSize: "0.92rem",
                fontWeight: "650",
              })}
            >
              Display name
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

            {props.validationError || props.validatedProject ? (
              <div
                class={css({
                  display: "flex",
                  flexDirection: "column",
                  gap: "10px",
                  padding: "16px",
                  borderRadius: "22px",
                  border: "1px solid",
                  borderColor: "border",
                  backgroundColor: "surface",
                })}
              >
                {props.validationError ? (
                  <div
                    class={css({
                      display: "flex",
                      alignItems: "center",
                      gap: "10px",
                      color: "danger",
                      fontSize: "0.88rem",
                      fontWeight: "620",
                    })}
                  >
                    <AlertCircle size={16} strokeWidth={2.1} />
                    {props.validationError}
                  </div>
                ) : null}
                {props.validatedProject ? (
                  <div
                    class={css({
                      display: "flex",
                      flexDirection: "column",
                      gap: "8px",
                    })}
                  >
                    <div
                      class={css({
                        display: "flex",
                        alignItems: "center",
                        gap: "10px",
                        color: "accentStrong",
                        fontSize: "0.86rem",
                        fontWeight: "650",
                      })}
                    >
                      <CheckCircle2 size={16} strokeWidth={2.1} />
                      Validated directory
                    </div>
                    <span
                      class={css({
                        color: "text",
                        fontWeight: "620",
                        lineHeight: "1.55",
                        wordBreak: "break-word",
                      })}
                    >
                      {props.validatedProject.path}
                    </span>
                  </div>
                ) : null}
              </div>
            ) : null}

            <div
              class={css({
                display: "flex",
                justifyContent: "flex-end",
                gap: "10px",
              })}
            >
              <Dialog.Close asChild>
                <button class={cx(buttonBaseClass, secondaryButtonClass)} type="button">
                  Cancel
                </button>
              </Dialog.Close>
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
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
