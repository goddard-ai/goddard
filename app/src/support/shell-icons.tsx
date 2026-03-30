import type { JSX } from "preact"

/** Icon names used by the sprint-1 shell components. */
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
  | "identity"

/**
 * Renders one lightweight inline SVG icon for the app shell without adding an icon dependency.
 */
export function ShellIcon(props: { name: ShellIconName; title?: string }): JSX.Element {
  const commonProps = {
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    strokeWidth: "1.65",
  }

  return (
    <svg
      aria-hidden={props.title ? undefined : true}
      role={props.title ? "img" : undefined}
      viewBox="0 0 20 20"
    >
      {props.title ? <title>{props.title}</title> : null}
      {props.name === "main" ? (
        <>
          <rect x="3" y="3.5" width="14" height="13" rx="2.5" {...commonProps} />
          <path d="M3 8h14" {...commonProps} />
        </>
      ) : null}
      {props.name === "projects" ? (
        <>
          <path d="M4.5 6.5h11l-1 8H5.5l-1-8Z" {...commonProps} />
          <path d="M7 6.5V5a1.5 1.5 0 0 1 1.5-1.5h3A1.5 1.5 0 0 1 13 5v1.5" {...commonProps} />
        </>
      ) : null}
      {props.name === "sessions" ? (
        <>
          <path d="M4 5.5h12v7H8l-3.5 2.5V5.5Z" {...commonProps} />
        </>
      ) : null}
      {props.name === "pullRequests" ? (
        <>
          <circle cx="5" cy="5" r="1.5" {...commonProps} />
          <circle cx="15" cy="15" r="1.5" {...commonProps} />
          <circle cx="15" cy="5" r="1.5" {...commonProps} />
          <path d="M6.5 5h5M15 6.5v7M5 6.5v7a2 2 0 0 0 2 2h6.5" {...commonProps} />
        </>
      ) : null}
      {props.name === "specs" ? (
        <>
          <path d="M5 3.5h7l3 3v10H5v-13Z" {...commonProps} />
          <path d="M12 3.5v3h3" {...commonProps} />
          <path d="M7.5 10h5M7.5 13h5" {...commonProps} />
        </>
      ) : null}
      {props.name === "tasks" ? (
        <>
          <rect x="4" y="4" width="12" height="12" rx="2.5" {...commonProps} />
          <path d="m7 10 2 2 4-4" {...commonProps} />
        </>
      ) : null}
      {props.name === "roadmap" ? (
        <>
          <path d="M4 15.5V4.5" {...commonProps} />
          <path d="M4 5h8l-1.5 3L12 11H4" {...commonProps} />
        </>
      ) : null}
      {props.name === "actions" ? (
        <>
          <path d="M7 4.5h6l2.5 2.5-6.5 8H4.5V10l2.5-5.5Z" {...commonProps} />
          <path d="M11.5 4.5 15.5 8.5" {...commonProps} />
        </>
      ) : null}
      {props.name === "loops" ? (
        <>
          <path d="M6 7a4.5 4.5 0 0 1 7.5-1.5L15 7" {...commonProps} />
          <path d="M14 13a4.5 4.5 0 0 1-7.5 1.5L5 13" {...commonProps} />
          <path d="M15 7V4.5h-2.5M5 13v2.5h2.5" {...commonProps} />
        </>
      ) : null}
      {props.name === "inbox" ? (
        <>
          <path d="M4 6.5h12l-1.5 9H5.5L4 6.5Z" {...commonProps} />
          <path d="M4 9.5h3l1.5 2h3L13 9.5h3" {...commonProps} />
        </>
      ) : null}
      {props.name === "identity" ? (
        <>
          <circle cx="10" cy="7" r="3" {...commonProps} />
          <path d="M4.5 16a5.5 5.5 0 0 1 11 0" {...commonProps} />
        </>
      ) : null}
    </svg>
  )
}
