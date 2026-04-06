import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { ShellIcon } from "~/support/shell-icons"
import { useProjectRegistry } from "~/app-state-context"
import { lookupProject } from "./project-registry"

const pageClass = css({
  display: "flex",
  flexDirection: "column",
  gap: "18px",
  height: "100%",
  padding: "28px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 32%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const cardClass = css({
  maxWidth: "880px",
  padding: "32px",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
  boxShadow: "0 28px 80px rgba(121, 138, 160, 0.14)",
})

const badgeClass = css({
  display: "inline-flex",
  alignItems: "center",
  gap: "8px",
  padding: "8px 12px",
  borderRadius: "999px",
  backgroundColor: "surface",
  marginBottom: "10px",
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "700",
  letterSpacing: "0.14em",
  textTransform: "uppercase",
})

const titleClass = css({
  marginBottom: "12px",
  color: "text",
  fontSize: "1.4rem",
  fontWeight: "700",
})

const bodyClass = css({
  color: "muted",
  lineHeight: "1.72",
})

/** Renders one project-backed closable workbench tab. */
export function ProjectPage(props: { projectPath: string }) {
  const projectRegistry = useProjectRegistry()
  const project = lookupProject(projectRegistry, props.projectPath)

  if (!project) {
    return (
      <div class={pageClass}>
        <section class={cardClass}>
          <h1 class={titleClass}>Project unavailable</h1>
          <p class={bodyClass}>
            The project record for this tab is no longer in the machine-wide project registry.
          </p>
        </section>
      </div>
    )
  }

  return (
    <div class={pageClass}>
      <section class={cardClass}>
        <div class={badgeClass}>
          <span class={css({ width: "14px", height: "14px" })}>
            <ShellIcon name="projects" />
          </span>
          Tab
        </div>
        <h1 class={titleClass}>{project.name}</h1>
        <p class={bodyClass}>
          This project tab keeps the primary workbench view and closable tabs alive side by side. It
          can later be replaced with richer project-scoped surfaces such as sessions, specs, tasks,
          and pull requests.
        </p>
        <p class={bodyClass}>Path: {project.path}</p>
      </section>
    </div>
  )
}

export default ProjectPage
