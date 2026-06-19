import { t } from "@lingui/core/macro"
import { Search } from "lucide-react"

import styles from "./list-toolbar.style.ts"

export function ListToolbar(props: {
  inputRef?: (element: HTMLInputElement | null) => void | (() => void);
  searchQuery: string;
}) @{
  <label class={styles.searchField}>
    <span class={styles.srOnly}>
      {t`Search session titles`}
    </span>
    <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
    <input
      ref={props.inputRef}
      class={styles.searchInput}
      placeholder={t`Search session titles`}
      type="search"
      value={props.searchQuery}
    />
  </label>
}
