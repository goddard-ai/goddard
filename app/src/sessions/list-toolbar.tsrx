import { Search } from "lucide-react"

import styles from "./list-toolbar.style.ts"

export function ListToolbar(props: {
  searchQuery: string;
  onSearchInput: (value: string) => void;
}) {
  return <tsx:react>
    <label class={styles.searchField}>
      <span class={styles.srOnly}>Search session titles</span>
      <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
      <input class={styles.searchInput} placeholder="Search session titles" value={props.searchQuery} onInput={(
        event,
      ) => {
        props.onSearchInput(event.currentTarget.value)
      }} />
    </label>
  </tsx:react>
}
