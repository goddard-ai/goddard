import { Search } from "lucide-react"

import { text } from "~/language/text.ts"
import styles from "./list-toolbar.style.ts"

export function ListToolbar(props: {
  inputRef?: (element: HTMLInputElement | null) => void | (() => void);
  searchQuery: string;
}) @{
  <label class={styles.searchField}>
    <span class={styles.srOnly}>{text.searchSessionTitles}</span>
    <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
    <input
      ref={props.inputRef}
      class={styles.searchInput}
      placeholder={text.searchSessionTitles}
      type="search"
      value={props.searchQuery}
    />
  </label>
}
