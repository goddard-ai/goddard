import { Trash2 } from "lucide-react"

import { useSavedPrompts } from "~/app-state-context.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import {
  SettingsFieldset,
  SettingsHelperText,
  SettingsLegend,
  SettingsSection,
  SettingsSectionDescription,
  SettingsSectionHeader,
  SettingsSectionTitle,
} from "~/settings/styled.tsrx"
import styles from "./settings-section.style.ts"

export const savedPromptsSettingsSectionMetadata = {
  description: "Manage transcript prompts saved for Tab completion in session inputs.",
  id: "settings-saved-prompts",
  title: "Saved prompts",
}

export function SavedPromptsSettingsSection() @{
  const savedPrompts = useSavedPrompts()
  const prompts = savedPrompts.promptList

  <SettingsSection
    id={savedPromptsSettingsSectionMetadata.id}
    aria-labelledby="settings-saved-prompts-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-saved-prompts-heading"
      >{savedPromptsSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{savedPromptsSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>Prompts</SettingsLegend>
      <SettingsHelperText>Most recently saved prompts are suggested first.</SettingsHelperText>

      @if (prompts.length === 0) {
        <div class={styles.emptyState}>
          <p class={styles.emptyTitle}>No saved prompts</p>
          <p class={styles.emptyText}>
            Save a user prompt from a chat transcript to show it here.
          </p>
        </div>
      } @else {
        <ul class={styles.promptList}>
          @for (const prompt of prompts) {
            <li class={styles.promptItem} key={prompt.id}>
              <p class={styles.promptText}>{prompt.text}</p>
              <GoodTooltip ariaLabel="Delete saved prompt" content="Delete saved prompt" side="top">
                <button
                  aria-label="Delete saved prompt"
                  class={styles.deleteButton}
                  type="button"
                  onClick={() => {
                    savedPrompts.remove(prompt.id)
                  }}
                >
                  <Trash2 size={14} strokeWidth={2.2} />
                </button>
              </GoodTooltip>
            </li>
          }
        </ul>
      }
    </SettingsFieldset>
  </SettingsSection>
}
