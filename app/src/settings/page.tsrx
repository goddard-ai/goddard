import { css } from "@goddard-ai/styled-system/css"

import { AppearanceSettingsSection } from "~/appearance/appearance-settings-section.tsrx"

export function SettingsPage() {
  return <tsx:react>
    <main class={css(
      {
        display: "grid",
        alignContent: "start",
        gap: "24px",
        minHeight: "100%",
        padding: "28px",
      },
    )}>
      <AppearanceSettingsSection />
    </main>
  </tsx:react>
}

export default SettingsPage
