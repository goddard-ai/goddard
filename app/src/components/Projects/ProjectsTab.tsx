import type { WorkbenchTabComponentProps } from "../state/WorkbenchTabRegistry"
import { ProjectsPage } from "./ProjectsPage"

/** Renders the projects primary surface inside one closable workbench tab. */
export default function ProjectsTab(props: WorkbenchTabComponentProps<"projects", {}>) {
  void props
  return <ProjectsPage />
}
