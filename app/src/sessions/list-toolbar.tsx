import { Search } from "lucide-react"

import styles from "./list-toolbar.style.ts"

export function ListToolbar(props: {
  searchQuery: string
  sessionCount: number
  visibleSessionCount: number
  onSearchInput: (value: string) => void
}) {
  const hasSearch = props.searchQuery.trim().length > 0

  return (
    <header class={styles.root}>
      <div class={styles.content}>
        <h1 class={styles.title}>Sessions</h1>
        <p class={styles.description}>
          {hasSearch
            ? `${props.visibleSessionCount} match${props.visibleSessionCount === 1 ? "" : "es"}`
            : `${props.sessionCount} session${props.sessionCount === 1 ? "" : "s"}`}
        </p>
      </div>
      <label class={styles.searchField}>
        <span class={styles.srOnly}>Search session titles</span>
        <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
        <input
          class={styles.searchInput}
          placeholder="Search session titles"
          value={props.searchQuery}
          onInput={(event) => {
            props.onSearchInput(event.currentTarget.value)
          }}
        />
      </label>
    </header>
  )
}
