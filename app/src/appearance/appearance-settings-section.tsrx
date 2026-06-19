import { t } from "@lingui/core/macro"
import { useAppearance } from "~/app-state-context.tsrx"
import {
  SettingsChoiceBody,
  SettingsChoiceDescription,
  SettingsChoiceInput,
  SettingsChoiceItem,
  SettingsChoiceLabel,
  SettingsChoiceList,
  SettingsChoiceTitle,
  SettingsControlRowLabel,
  SettingsFieldset,
  SettingsHelperText,
  SettingsLegend,
  SettingsSection,
  SettingsSectionDescription,
  SettingsSectionHeader,
  SettingsSectionTitle,
} from "~/settings/styled.tsrx"
import styles from "./appearance-settings-section.style.ts"
import type { AppearanceMode } from "./theme.ts"

export const appearanceSettingsSectionMetadata = {
  id: "settings-appearance",
  get description() {
    return t`Choose how the workbench resolves theme and contrast.`
  },
  get title() {
    return t`Appearance`
  },
}

const themeModeOptions: readonly {
  mode: AppearanceMode;
  label: string;
  description: string;
}[] = [
  {
    mode: "system",
    get label() {
      return t`System`
    },
    get description() {
      return t`Follow the operating system appearance.`
    },
  },
  {
    mode: "light",
    get label() {
      return t`Light`
    },
    get description() {
      return t`Use the light built-in palette.`
    },
  },
  {
    mode: "dark",
    get label() {
      return t`Dark`
    },
    get description() {
      return t`Use the dark built-in palette.`
    },
  },
]

export function AppearanceSettingsSection() @{
  const appearance = useAppearance()

  <SettingsSection
    id={appearanceSettingsSectionMetadata.id}
    aria-labelledby="settings-appearance-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle
        id="settings-appearance-heading"
      >{appearanceSettingsSectionMetadata.title}</SettingsSectionTitle>
      <SettingsSectionDescription>{appearanceSettingsSectionMetadata.description}</SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>
        {t`Color mode`}
      </SettingsLegend>
      <SettingsHelperText>
        {t`Current effective theme:`}
        {" "}
        {appearance.effectiveTheme}
      </SettingsHelperText>
      <SettingsChoiceList>
        @for (const option of themeModeOptions) {
          <SettingsChoiceItem key={option.mode}>
            <SettingsChoiceLabel
              class={styles.choiceLabel}
              data-selected={appearance.mode === option.mode ? "true" : undefined}
            >
              <SettingsChoiceBody>
                <SettingsChoiceTitle>{option.label}</SettingsChoiceTitle>
                <SettingsChoiceDescription>{option.description}</SettingsChoiceDescription>
              </SettingsChoiceBody>
              <SettingsChoiceInput
                checked={appearance.mode === option.mode}
                name="appearance-mode"
                type="radio"
                value={option.mode}
                onInput={() => {
                  appearance.setMode(option.mode)
                }}
              />
            </SettingsChoiceLabel>
          </SettingsChoiceItem>
        }
      </SettingsChoiceList>
    </SettingsFieldset>
    <SettingsFieldset>
      <SettingsLegend>
        {t`Contrast`}
      </SettingsLegend>
      <SettingsHelperText>
        {t`Tightens muted text and border separation across the app.`}
      </SettingsHelperText>
      <SettingsControlRowLabel
        class={styles.controlRow}
        data-selected={appearance.highContrast ? "true" : undefined}
      >
        <SettingsChoiceBody>
          <SettingsChoiceTitle>
            {t`High contrast`}
          </SettingsChoiceTitle>
          <SettingsChoiceDescription>
            {appearance.highContrast ? t`Enabled` : t`Disabled`}
          </SettingsChoiceDescription>
        </SettingsChoiceBody>
        <SettingsChoiceInput
          checked={appearance.highContrast}
          class={styles.switchInput}
          type="checkbox"
          onInput={(event) => {
            appearance.setHighContrast(event.currentTarget.checked)
          }}
        />
        <span class={styles.switchTrack} aria-hidden={true}>
          <span class={styles.switchThumb} />
        </span>
      </SettingsControlRowLabel>
    </SettingsFieldset>
  </SettingsSection>
}
