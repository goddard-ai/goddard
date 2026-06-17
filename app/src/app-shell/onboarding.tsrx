import { FolderOpen } from "lucide-react"

import { AppCommand } from "~/commands/app-command.ts"
import { text } from "~/language/text.ts"
import styles from "./onboarding.style.ts"

/** First-run surface shown while the workspace has no user-added projects. */
export function AppShellOnboarding() @{
  <main class={styles.root}>
    <section class={styles.content} aria-labelledby="empty-workspace-title">
      <div class={styles.copy}>
        <h1 class={styles.title} id="empty-workspace-title">{text.openAProject}</h1>
        <p class={styles.description}>{text.chooseALocalFolderToStartWorkingInGoddard}</p>
      </div>
      <button
        class={styles.button}
        type="button"
        onClick={() => {
          AppCommand.projects.openFolder()
        }}
      >
        <FolderOpen aria-hidden={true} size={17} strokeWidth={2.2} />
        {text.openProject}
      </button>
    </section>
  </main>
}
