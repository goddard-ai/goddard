import { useProjectRegistry } from "~/app-state-context.tsrx"
import { GoodIcon } from "~/lib/good-icon.tsrx"
import styles from "./project-page.style.ts"
import { lookupProject } from "./project-registry.ts"

/** Renders one project-backed closable workbench tab. */
export function ProjectPage(props: { projectPath: string }) {
  const projectRegistry = useProjectRegistry()
  const project = lookupProject(projectRegistry, props.projectPath)

  if (!project) {
    return <tsx:react>
      <div class={styles.page}>
        <section class={styles.card}>
          <h1 class={styles.title}>Project unavailable</h1>
          <p class={styles.body}>The project record for this tab is no longer in the machine-wide
            project registry.</p>
        </section>
      </div>
    </tsx:react>
  }

  return <tsx:react>
    <div class={styles.page}>
      <section class={styles.card}>
        <div class={styles.badge}>
          <span class={styles.icon}>
            <GoodIcon name="tabs/projects" height="14px" width="14px" />
          </span>
          Tab
        </div>
        <h1 class={styles.title}>
          {project.name}
        </h1>
        <p class={styles.body}>This project tab keeps the primary workbench view and closable tabs
          alive side by side. It can later be replaced with richer
          project-scoped surfaces such as sessions, specs, tasks, and pull
          requests.</p>
        <p class={styles.body}>
          Path:
          {project.path}
        </p>
      </section>
    </div>
  </tsx:react>
}

export default ProjectPage
