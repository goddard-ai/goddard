import { useSignal } from "@preact/signals"
import { useMutation } from "@tanstack/preact-query"
import { useEffect } from "preact/hooks"
import { css } from "../../styled-system/css"
import { token } from "../../styled-system/tokens"
import { useProjectRegistry, useWorkbenchTabSet } from "../state/app-context"
import { lookupProject, type ProjectRecord } from "../state/project-registry"
import { pickProjectPath, validateProjectPath } from "../support/project-service"

const cardClass = css({
  display: "flex",
  flexDirection: "column",
  minHeight: "0",
  border: "1px solid",
  borderColor: "border",
  borderRadius: "20px",
  backgroundColor: "panel",
  boxShadow: "0 18px 40px rgba(124, 143, 166, 0.08)",
})

const eyebrowClass = css({
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "700",
  letterSpacing: "0.14em",
  textTransform: "uppercase",
})

const titleClass = css({
  color: "text",
  fontSize: "1.35rem",
  fontWeight: "700",
  letterSpacing: "-0.02em",
})

const bodyClass = css({
  color: "muted",
  fontSize: "0.95rem",
  lineHeight: "1.6",
})

const rowClass = css({
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto",
  gap: "16px",
  padding: "16px",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "surface",
  transition: "border-color 140ms ease, background-color 140ms ease, box-shadow 140ms ease",
  cursor: "pointer",
  "&[data-selected='true']": {
    backgroundColor: "background",
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
  },
})

const rowMetaClass = css({
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  minWidth: "0",
})

const rowTitleClass = css({
  color: "text",
  fontSize: "0.98rem",
  fontWeight: "700",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
})

const rowPathClass = css({
  color: "muted",
  fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
  fontSize: "0.82rem",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
})

const rowActionsClass = css({
  display: "flex",
  alignItems: "flex-start",
  gap: "8px",
})

const ghostButtonClass = css({
  height: "34px",
  paddingInline: "12px",
  borderRadius: "999px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  color: "text",
  cursor: "pointer",
  fontSize: "0.82rem",
  fontWeight: "600",
  _hover: {
    borderColor: "accent",
  },
})

const statusBlockClass = css({
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  padding: "14px",
  borderRadius: "16px",
  backgroundColor: "surface",
  border: "1px solid",
  borderColor: "border",
})

const fieldLabelClass = css({
  display: "flex",
  flexDirection: "column",
  gap: "8px",
  color: "text",
  fontSize: "0.9rem",
  fontWeight: "600",
})

const textInputClass = css({
  width: "100%",
  height: "44px",
  paddingInline: "14px",
  borderRadius: "14px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "surface",
  color: "text",
  fontSize: "0.95rem",
  outline: "none",
  _focusVisible: {
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 18%, transparent)`,
  },
  _disabled: {
    opacity: "0.7",
  },
})

/** Renders the sprint-1 projects page plus its local add-project modal flow. */
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
  const validationError =
    projectValidation.error instanceof Error
      ? projectValidation.error.message
      : (projectValidation.error?.toString() ?? null)
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

  useEffect(() => {
    if (!isAddDialogOpen.value) {
      return
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeAddDialog()
      }
    }

    window.addEventListener("keydown", handleKeyDown)
    return () => {
      window.removeEventListener("keydown", handleKeyDown)
    }
  }, [isAddDialogOpen.value])

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
        gridTemplateColumns: "minmax(0, 2fr) minmax(280px, 1fr)",
        gap: "20px",
        height: "100%",
        padding: "24px",
        backgroundColor: "background",
        "@media (max-width: 960px)": {
          gridTemplateColumns: "1fr",
        },
      })}
    >
      <section class={cardClass}>
        <header
          class={css({
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: "16px",
            padding: "22px 24px 18px",
            borderBottom: "1px solid",
            borderColor: "border",
          })}
        >
          <div
            class={css({
              display: "flex",
              flexDirection: "column",
              gap: "6px",
            })}
          >
            <span class={eyebrowClass}>Machine-Wide Scope</span>
            <h1 class={titleClass}>Projects</h1>
            <p class={bodyClass}>
              Keep an explicit working set of local project roots for discovery, session launch,
              specs, tasks, and later review workflows.
            </p>
          </div>
          <button
            class={css({
              height: "38px",
              paddingInline: "16px",
              borderRadius: "999px",
              border: "1px solid",
              borderColor: "accent",
              backgroundColor: "surface",
              color: "text",
              cursor: "pointer",
              fontWeight: "600",
              transition: "background-color 140ms ease, border-color 140ms ease",
              _hover: {
                backgroundColor: "background",
                borderColor: "accentStrong",
              },
            })}
            type="button"
            onClick={() => {
              openAddDialog()
            }}
          >
            Add project
          </button>
        </header>

        {projects.length === 0 ? (
          <div
            class={css({
              display: "grid",
              placeItems: "center",
              minHeight: "280px",
              padding: "32px",
              textAlign: "center",
              color: "muted",
            })}
          >
            <div>
              <h2 class={titleClass}>No projects yet</h2>
              <p class={bodyClass}>
                Add one local directory to establish the app&apos;s explicit machine-wide project
                scope.
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

      <aside class={cardClass}>
        <div
          class={css({
            display: "flex",
            flexDirection: "column",
            gap: "18px",
            padding: "24px",
          })}
        >
          <span class={eyebrowClass}>Inspector</span>
          {selectedProject ? (
            <>
              <h2 class={titleClass}>{selectedProject.name}</h2>
              <p class={bodyClass}>
                Selected project record for the current machine-wide workspace scope.
              </p>
              <div class={statusBlockClass}>
                <span class={bodyClass}>Path</span>
                <span class={css({ color: "text", fontWeight: "600" })}>
                  {selectedProject.path}
                </span>
              </div>
              <div class={statusBlockClass}>
                <span class={bodyClass}>Project name</span>
                <span class={css({ color: "text", fontWeight: "600" })}>
                  {selectedProject.name}
                </span>
              </div>
              <p class={css({ color: "muted", lineHeight: "1.65" })}>
                Projects are just explicit local directory roots. Git-specific checks and deeper
                capability discovery can be layered in later when a concrete screen needs them.
              </p>
            </>
          ) : (
            <>
              <h2 class={titleClass}>No project selected</h2>
              <p class={bodyClass}>
                Select one project row to inspect its path and configured display name.
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
        padding: "12px",
        gap: "10px",
        minHeight: "0",
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
    <article
      class={rowClass}
      data-selected={props.isSelected}
      onClick={() => {
        props.onSelect(props.project.path)
      }}
    >
      <div class={rowMetaClass}>
        <h3 class={rowTitleClass}>{props.project.name}</h3>
        <p class={rowPathClass}>{props.project.path}</p>
      </div>
      <div class={rowActionsClass}>
        <button
          class={ghostButtonClass}
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            props.onOpenProjectTab(props.project.path)
          }}
        >
          Open tab
        </button>
        <button
          class={ghostButtonClass}
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            props.onRemove(props.project.path)
          }}
        >
          Remove
        </button>
      </div>
    </article>
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
  if (!props.isAddDialogOpen) {
    return null
  }

  return (
    <div
      class={css({
        position: "fixed",
        inset: "0",
        display: "grid",
        placeItems: "center",
        padding: "24px",
        backgroundColor: `color-mix(in srgb, ${token.var("colors.accentStrong")} 10%, rgba(24, 33, 43, 0.28))`,
        backdropFilter: "blur(8px)",
      })}
    >
      <section
        aria-labelledby="add-project-title"
        aria-modal="true"
        class={css({
          width: "min(560px, 100%)",
          borderRadius: "24px",
          border: "1px solid",
          borderColor: "border",
          backgroundColor: "background",
          boxShadow: "0 30px 80px rgba(76, 97, 122, 0.18)",
        })}
        role="dialog"
      >
        <div
          class={css({
            display: "flex",
            flexDirection: "column",
            gap: "18px",
            padding: "24px",
          })}
        >
          <div>
            <span class={eyebrowClass}>Add Project</span>
            <h2 class={titleClass} id="add-project-title">
              Choose one local root
            </h2>
            <p class={bodyClass}>
              Project validation stays local to this dialog. The shared project registry only keeps
              the path and display name.
            </p>
          </div>

          <label class={fieldLabelClass}>
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
              class={ghostButtonClass}
              type="button"
              onClick={() => {
                void props.browseForProject()
              }}
            >
              Browse
            </button>
            <button
              class={ghostButtonClass}
              disabled={props.draftPath.trim().length === 0 || props.isValidating}
              type="button"
              onClick={() => {
                void props.inspectDraftPath(props.draftPath)
              }}
            >
              {props.isValidating ? "Validating..." : "Validate"}
            </button>
          </div>

          <label class={fieldLabelClass}>
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
            <div class={statusBlockClass}>
              {props.validationError ? (
                <span
                  class={css({
                    color: "danger",
                    fontSize: "0.82rem",
                    fontWeight: "600",
                  })}
                >
                  {props.validationError}
                </span>
              ) : null}
              {props.validatedProject ? (
                <>
                  <span class={bodyClass}>Validated directory</span>
                  <span class={css({ color: "text", fontWeight: "600" })}>
                    {props.validatedProject.path}
                  </span>
                </>
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
            <button
              class={ghostButtonClass}
              type="button"
              onClick={() => {
                props.closeAddDialog()
              }}
            >
              Cancel
            </button>
            <button
              class={ghostButtonClass}
              disabled={!props.canAddProject}
              type="button"
              onClick={() => {
                props.addProject()
              }}
            >
              Add project
            </button>
          </div>
        </div>
      </section>
    </div>
  )
}
