import { useState } from "preact/hooks"
import { Search } from "lucide-react"

import {
  appearanceSettingsSectionMetadata,
  AppearanceSettingsSection,
} from "~/appearance/appearance-settings-section.tsrx"
import { text } from "~/language/text.ts"
import {
  adapterInstallationsSettingsSectionMetadata,
  AdapterInstallationsSettingsSection,
} from "./adapter-installations-section.tsrx"
import { filterPreparedSettingsSearchSections, prepareSettingsSearchSections } from "./search.ts"
import styles from "./page.style.ts"

const settingsSections = [
  {
    ...appearanceSettingsSectionMetadata,
    href: `#${appearanceSettingsSectionMetadata.id}`,
  },
  {
    ...adapterInstallationsSettingsSectionMetadata,
    href: `#${adapterInstallationsSettingsSectionMetadata.id}`,
  },
]

const preparedSettingsSections = prepareSettingsSearchSections(settingsSections)

export default function SettingsPage() @{
  const [searchQuery, setSearchQuery] = useState("")
  const visibleSections = filterPreparedSettingsSearchSections(
    preparedSettingsSections,
    searchQuery,
  )

  <main class={styles.page}>
    <label class={styles.searchField}>
      <span class={styles.srOnly}>{text.searchSettings}</span>
      <Search aria-hidden={true} class={styles.searchIcon} size={16} strokeWidth={2.1} />
      <input
        class={styles.searchInput}
        placeholder={text.searchSettings}
        role="searchbox"
        type="search"
        value={searchQuery}
        onInput={(event) => {
          setSearchQuery(event.currentTarget.value)
        }}
      />
    </label>
    <div class={styles.content}>
      @for (const section of visibleSections) {
        @if (section.id === appearanceSettingsSectionMetadata.id) {
          <AppearanceSettingsSection key={section.id} />
        } @else if (section.id === adapterInstallationsSettingsSectionMetadata.id) {
          <AdapterInstallationsSettingsSection key={section.id} />
        } @else {
          <></>
        }
      }

      @if (visibleSections.length === 0) {
        <section class={styles.emptyState} aria-live="polite">
          <h2 class={styles.emptyTitle}>{text.noMatchingSettings}</h2>
          <p class={styles.emptyText}>{text.tryADifferentSearchTerm}</p>
        </section>
      }
    </div>

    @if (visibleSections.length > 1) {
      <aside class={styles.sidebar} aria-label={text.settingsSections}>
        <h2 class={styles.sidebarTitle}>{text.settings}</h2>
        <ul class={styles.sidebarList}>
          @for (const section of visibleSections) {
            <li key={section.href}>
              <a class={styles.sidebarLink} href={section.href}>{section.title}</a>
            </li>
          }
        </ul>
      </aside>
    }
  </main>
}
