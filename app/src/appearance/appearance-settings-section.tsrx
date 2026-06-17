import { useAppearance } from "~/app-state-context.tsrx"
import { text } from "~/language/text.ts"
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
  description: text.chooseHowTheWorkbenchResolvesThemeAndContrast,
  id: "settings-appearance",
  title: text.appearance,
}

const themeModeOptions: readonly {
  mode: AppearanceMode;
  label: string;
  description: string;
}[] = [
  {
    mode: "system",
    label: text.system,
    description: text.followTheOperatingSystemAppearance,
  },
  {
    mode: "light",
    label: text.light,
    description: text.useTheLightBuiltInPalette,
  },
  {
    mode: "dark",
    label: text.dark,
    description: text.useTheDarkBuiltInPalette,
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
      <SettingsLegend>{text.colorMode}</SettingsLegend>
      <SettingsHelperText>
        {text.currentEffectiveTheme}
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
      <SettingsLegend>{text.contrast}</SettingsLegend>
      <SettingsHelperText>{text.tightensMutedTextAndBorderSeparationAcrossTheApp}</SettingsHelperText>
      <SettingsControlRowLabel
        class={styles.controlRow}
        data-selected={appearance.highContrast ? "true" : undefined}
      >
        <SettingsChoiceBody>
          <SettingsChoiceTitle>{text.highContrast}</SettingsChoiceTitle>
          <SettingsChoiceDescription>
            {appearance.highContrast ? text.enabled : text.disabled}
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
