import { useState } from "preact/hooks"
import { Search } from "lucide-react"

import {
  appearanceSettingsSectionMetadata,
  AppearanceSettingsSection,
} from "~/appearance/appearance-settings-section.tsrx"
import { filterPreparedSettingsSearchSections, prepareSettingsSearchSections } from "./search.ts"
import styles from "./page.style.ts"

const settingsSections = [
  {
    ...appearanceSettingsSectionMetadata,
    href: `#${appearanceSettingsSectionMetadata.id}`,
  },
]

const preparedSettingsSections = prepareSettingsSearchSections(settingsSections)

export component SettingsPage() {
  const [searchQuery, setSearchQuery] = useState("")
  const visibleSections = filterPreparedSettingsSearchSections(
    preparedSettingsSections,
    searchQuery,
  )

  <main class={styles.page}>
    <label class={styles.searchField}>
      <span class={styles.srOnly}>"Search settings"</span>
      <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
      <input
        class={styles.searchInput}
        placeholder="Search settings"
        role="searchbox"
        type="search"
        value={searchQuery}
        onInput={(event) => {
          setSearchQuery(event.currentTarget.value)
        }}
      />
    </label>
    <div class={styles.content}>
      for (const section of visibleSections) {
        if (section.id === appearanceSettingsSectionMetadata.id) {
          <AppearanceSettingsSection key={section.id} />
        }
      }
    </div>
    if (visibleSections.length > 0) {
      <aside class={styles.sidebar} aria-label="Settings sections">
        <h2 class={styles.sidebarTitle}>"Settings"</h2>
        <ul class={styles.sidebarList}>
          for (const section of visibleSections) {
            <li key={section.href}>
              <a class={styles.sidebarLink} href={section.href}>
                {section.title}
              </a>
            </li>
          }
        </ul>
      </aside>
    }
  </main>
}

export default SettingsPage
