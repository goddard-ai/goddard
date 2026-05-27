import { AppearanceSettingsSection } from "~/appearance/appearance-settings-section.tsrx"
import styles from "./page.style.ts"

const settingsSections = [
  {
    href: "#settings-appearance",
    label: "Appearance",
  },
]

export component SettingsPage() {
  <main class={styles.page}>
    <div class={styles.content}>
      <AppearanceSettingsSection />
    </div>
    <aside class={styles.sidebar} aria-label="Settings sections">
      <h2 class={styles.sidebarTitle}>"Settings"</h2>
      <ul class={styles.sidebarList}>
        for (const section of settingsSections) {
          <li key={section.href}>
            <a class={styles.sidebarLink} href={section.href}>
              {section.label}
            </a>
          </li>
        }
      </ul>
    </aside>
  </main>
}

export default SettingsPage
