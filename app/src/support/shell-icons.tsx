import {
  FileText,
  FolderKanban,
  Inbox,
  LayoutPanelLeft,
  Map,
  MessageSquare,
  Search,
  SquareCheckBig,
  type LucideIcon,
} from "lucide-react"

/** Icon names used by the app shell components. */
export type ShellIconName =
  | "main"
  | "projects"
  | "sessions"
  | "search"
  | "specs"
  | "tasks"
  | "roadmap"
  | "inbox"

const shellIcons: Record<ShellIconName, LucideIcon> = {
  main: LayoutPanelLeft,
  projects: FolderKanban,
  sessions: MessageSquare,
  search: Search,
  specs: FileText,
  tasks: SquareCheckBig,
  roadmap: Map,
  inbox: Inbox,
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
