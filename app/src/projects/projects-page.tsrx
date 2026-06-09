import { cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { type Signal, useSignal } from "@preact/signals"
import { ArrowUpRight, FolderSearch2, Plus, Trash2, X } from "lucide-react"
import { useEffect, useId, useRef } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { browseForProject as browseForProjectPath } from "~/desktop-host.ts"
import { Modal } from "@goddard-ai/ui-primitives"
import { useMutations } from "~/lib/mutations-provider.tsx"
import { openProjectTab, ProjectsPageMutations } from "./mutations.ts"
import { deriveProjectName } from "./project-name.ts"
import type { ProjectRecord } from "./project-registry.ts"
import styles from "./projects-page.style.ts"

/** Renders the projects page plus its local add-project modal flow. */
export function ProjectsPage() @{
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const isAddProjectOpen = useSignal(false)
  const selectedProjectPath = useSignal<string | null>(projectRegistry.projectList[0]?.path ?? null)
  const projects = projectRegistry.projectList
  const resolvedSelectedProjectPath =
    selectedProjectPath.value && projectRegistry.projectsByPath[selectedProjectPath.value]
      ? selectedProjectPath.value
      : projects[0]?.path ?? null
  const selectedProject = resolvedSelectedProjectPath
    ? projectRegistry.projectsByPath[resolvedSelectedProjectPath] ?? null
    : null

  function selectProject(projectPath: string): void {
    selectedProjectPath.value = projectPath
  }

  function removeProject(projectPath: string): void {
    projectContext.removeProject(projectPath)
    projectRegistry.removeProject(projectPath)
  }

  function openProjectInTab(projectPath: string): void {
    openProjectTab({
      projectContext,
      projectPath,
      projectRegistry,
      workbenchTabSet,
    })
  }

  <ProjectsPageMutations
    mutations={{
      openProjectTab: openProjectInTab,
      removeProject,
      selectProject,
    }}
  >
    <div class={styles.root}>
      <section class={styles.panel}>
        <header class={styles.sectionHeader}>
          <div class={styles.sectionCopy}>
            <h1 class={styles.title}>Projects</h1>
            <p class={styles.body}>
              Keep an explicit set of local roots available to the workbench.
            </p>
          </div>
          <button
            class={cx(styles.buttonBase, styles.primaryButton)}
            type="button"
            onClick={() => {
              isAddProjectOpen.value = true
            }}
          >
            <Plus size={16} strokeWidth={2.2} />
            Add project
          </button>
        </header>

        @if (projects.length === 0) {
          <div class={styles.empty}>
            <div class={styles.emptyCopy}>
              <h2 class={styles.title}>No projects yet</h2>
              <p class={styles.body}>
                Add a local directory to establish the app's explicit working set.
              </p>
              <div>
                <button
                  class={cx(styles.buttonBase, styles.primaryButton)}
                  type="button"
                  onClick={() => {
                    isAddProjectOpen.value = true
                  }}
                >
                  <Plus size={16} strokeWidth={2.2} />
                  Add project
                </button>
              </div>
            </div>
          </div>
        } @else {
          <ProjectList {projects} selectedProjectPath={resolvedSelectedProjectPath} />
        }
      </section>
      <aside class={styles.panel}>
        <div class={styles.asideBody}>
          <div class={styles.intro}>
            <h2 class={styles.subheading}>Project details</h2>
            <p class={styles.body}>
              {selectedProject
                ? "Selected project metadata for the current workspace."
                : "Select a project to inspect its configured path and display name."}
            </p>
          </div>

          @if (selectedProject) {
            <>
              <h3 class={styles.selectedTitle}>{selectedProject.name}</h3>
              <div class={styles.cardList}>
                <InfoCard label="Path" value={selectedProject.path} />
                <InfoCard label="Display name" value={selectedProject.name} />
              </div>
            </>
          }
        </div>
      </aside>
      <AddProjectDialog open={isAddProjectOpen} {selectedProjectPath} />
    </div>
  </ProjectsPageMutations>
}

export default ProjectsPage

/** Renders the machine-wide project list. */
function ProjectList(props: {
  projects: readonly ProjectRecord[];
  selectedProjectPath: string | null;
}) @{
  <ul class={styles.list}>
    @for (const project of props.projects) {
      <li key={project.path}>
        <ProjectListRow isSelected={props.selectedProjectPath === project.path} {project} />
      </li>
    }
  </ul>
}

/** Renders one project row with quick actions and identity metadata. */
function ProjectListRow(props: {
  project: ProjectRecord;
  isSelected: boolean;
}) @{
  const { openProjectTab, removeProject, selectProject } = useMutations(ProjectsPageMutations)

  <article
    class={styles.row}
    style={{
      borderInlineStartColor: props.isSelected ? token.var("colors.accent") : "transparent",
      backgroundColor: props.isSelected ? token.var("colors.surface") : "transparent",
    }}
  >
    <button
      class={styles.rowButton}
      type="button"
      onClick={() => {
        selectProject(props.project.path)
      }}
    >
      <div class={styles.rowHeader}>
        <h3 class={styles.rowTitle}>{props.project.name}</h3>

        @if (props.isSelected) {
          <span class={styles.selectedBadge}>Selected</span>
        }
      </div>
      <p class={styles.path}>{props.project.path}</p>
    </button>
    <div class={styles.rowActions}>
      <button
        class={cx(styles.buttonBase, styles.secondaryButton)}
        type="button"
        onClick={(event) => {
          event.stopPropagation()
          openProjectTab(props.project.path)
        }}
      >
        <ArrowUpRight size={15} strokeWidth={2.2} />
        Open tab
      </button>
      <button
        class={cx(styles.buttonBase, styles.dangerButton)}
        type="button"
        onClick={(event) => {
          event.stopPropagation()
          removeProject(props.project.path)
        }}
      >
        <Trash2 size={15} strokeWidth={2.1} />
        Remove
      </button>
    </div>
  </article>
}

/** Renders one small inspector card. */
function InfoCard(props: { label: string; value: string }) @{
  <dl class={styles.infoCard}>
    <dt class={styles.quietLabel}>{props.label}</dt>
    <dd class={styles.infoValue}>{props.value}</dd>
  </dl>
}

/** Renders the modal flow used to add one project. */
function AddProjectDialog(props: {
  open: Signal<boolean>;
  selectedProjectPath: Signal<string | null>;
}) @{
  const projectRegistry = useProjectRegistry()

  const draftPath = useSignal("")
  const draftName = useSignal("")
  const lastSuggestedName = useSignal<string | null>(null)
  const titleId = useId()
  const descriptionId = useId()
  const pathInputRef = useRef<HTMLInputElement | null>(null)

  const draftProjectPath = draftPath.value.trim()
  const draftProjectName = draftName.value.trim()
  const derivedProjectName = deriveProjectName(draftPath.value)
  const canAddProject =
    draftProjectPath.length > 0 && (draftProjectName.length > 0 || derivedProjectName.length > 0)

  function resetDraft(): void {
    draftPath.value = ""
    draftName.value = ""
    lastSuggestedName.value = null
  }

  function updateDraftPath(value: string): void {
    draftPath.value = value
    const suggestedName = deriveProjectName(value)

    if (draftName.value.length === 0 || draftName.value === lastSuggestedName.value) {
      draftName.value = suggestedName
    }

    lastSuggestedName.value = suggestedName
  }

  function updateDraftName(value: string): void {
    draftName.value = value
  }

  async function browseForProject(): Promise<void> {
    const selectedPath = await browseForProjectPath()

    if (!selectedPath) {
      return
    }

    updateDraftPath(selectedPath)
  }

  function addProject(): void {
    if (draftProjectPath.length === 0) {
      return
    }

    const projectName = draftProjectName || deriveProjectName(draftProjectPath)

    if (projectName.length === 0) {
      return
    }

    projectRegistry.addProject({
      path: draftProjectPath,
      name: projectName,
    })
    props.selectedProjectPath.value = draftProjectPath
    props.open.value = false
    resetDraft()
  }

  useEffect(() => {
    if (!props.open.value) {
      return
    }

    resetDraft()
  }, [props.open.value])

  <Modal
    open={props.open}
    {titleId}
    {descriptionId}
    backdropClass={styles.dialogOverlay}
    positionerClass={styles.dialogPositioner}
    contentClass={styles.dialogContent}
    initialFocus={() => pathInputRef.current}
  >
    <form
      class={styles.form}
      onSubmit={(event) => {
        event.preventDefault()
        addProject()
      }}
    >
      <header class={styles.dialogHeader}>
        <div class={styles.dialogHeaderCopy}>
          <h2 id={titleId} class={styles.dialogTitle}>Add project</h2>
          <p id={descriptionId} class={styles.body}>
            Choose one local root and optionally adjust the display name.
          </p>
        </div>
        <button
          class={styles.closeButton}
          type="button"
          onClick={() => {
            props.open.value = false
          }}
        >
          <X size={15} strokeWidth={2.2} />
        </button>
      </header>
      <label class={styles.field}>
        <span class={styles.label}>Project path</span>
        <input
          ref={pathInputRef}
          class={styles.textInput}
          placeholder="/Users/alec/dev/goddard-ai/app"
          type="text"
          value={draftPath.value}
          onInput={(event) => {
            updateDraftPath(event.currentTarget.value)
          }}
        />
      </label>
      <div class={styles.browseRow}>
        <button
          class={cx(styles.buttonBase, styles.secondaryButton)}
          type="button"
          onClick={() => {
            void browseForProject()
          }}
        >
          <FolderSearch2 size={15} strokeWidth={2.1} />
          Browse
        </button>
      </div>
      <label class={styles.field}>
        <span class={styles.label}>Display name</span>
        <input
          class={styles.textInput}
          placeholder="Derived from folder name"
          type="text"
          value={draftName.value}
          onInput={(event) => {
            updateDraftName(event.currentTarget.value)
          }}
        />
      </label>
      <div class={styles.dialogActions}>
        <button
          class={cx(styles.buttonBase, styles.secondaryButton)}
          type="button"
          onClick={() => {
            props.open.value = false
          }}
        >Cancel</button>
        <button
          class={cx(styles.buttonBase, styles.primaryButton)}
          disabled={!canAddProject}
          type="button"
          onClick={() => {
            addProject()
          }}
        >
          <Plus size={15} strokeWidth={2.2} />
          Add project
        </button>
      </div>
    </form>
  </Modal>
}
