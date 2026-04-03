import {
  FileText,
  FolderKanban,
  GitPullRequest,
  Inbox,
  LayoutPanelLeft,
  ListTodo,
  Map,
  MessageSquare,
  Orbit,
  Settings2,
  SquareCheckBig,
  UserRound,
  WandSparkles,
  type LucideIcon,
} from "lucide-react"

/** Icon names used by the app shell components. */
export type ShellIconName =
  | "main"
  | "projects"
  | "sessions"
  | "pullRequests"
  | "specs"
  | "tasks"
  | "roadmap"
  | "actions"
  | "loops"
  | "inbox"
  | "settings"
  | "identity"

const shellIcons: Record<ShellIconName, LucideIcon> = {
  main: LayoutPanelLeft,
  projects: FolderKanban,
  sessions: MessageSquare,
  pullRequests: GitPullRequest,
  specs: FileText,
  tasks: SquareCheckBig,
  roadmap: Map,
  actions: WandSparkles,
  loops: Orbit,
  inbox: Inbox,
  settings: Settings2,
  identity: UserRound,
}

/** Renders one Lucide icon for the app shell. */
export function ShellIcon(props: { name: ShellIconName; title?: string }) {
  const Icon = shellIcons[props.name]

  return (
    <Icon
      aria-hidden={props.title ? undefined : true}
      aria-label={props.title}
      role={props.title ? "img" : undefined}
      size="100%"
      strokeWidth={1.85}
    />
  )
}
