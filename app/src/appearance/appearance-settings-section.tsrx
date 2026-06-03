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
import type { AppearanceMode } from "./theme.ts"

export const appearanceSettingsSectionMetadata = {
  description: "Choose how the workbench resolves theme and contrast.",
  id: "settings-appearance",
  title: "Appearance",
}

const themeModeOptions: readonly {
  mode: AppearanceMode;
  label: string;
  description: string;
}[] = [
  {
    mode: "system",
    label: "System",
    description: "Follow the operating system appearance.",
  },
  {
    mode: "light",
    label: "Light",
    description: "Use the light built-in palette.",
  },
  {
    mode: "dark",
    label: "Dark",
    description: "Use the dark built-in palette.",
  },
]

export component AppearanceSettingsSection() {
  const appearance = useAppearance()

  <SettingsSection
    id={appearanceSettingsSectionMetadata.id}
    aria-labelledby="settings-appearance-heading"
  >
    <SettingsSectionHeader>
      <SettingsSectionTitle id="settings-appearance-heading">
        {appearanceSettingsSectionMetadata.title}
      </SettingsSectionTitle>
      <SettingsSectionDescription>
        {appearanceSettingsSectionMetadata.description}
      </SettingsSectionDescription>
    </SettingsSectionHeader>
    <SettingsFieldset>
      <SettingsLegend>"Color mode"</SettingsLegend>
      <SettingsHelperText>
        "Current effective theme: "
        {appearance.effectiveTheme}
      </SettingsHelperText>
      <SettingsChoiceList>
        for (const option of themeModeOptions) {
          <SettingsChoiceItem key={option.mode}>
            <SettingsChoiceLabel
              data-selected={appearance.mode === option.mode ? "true" : undefined}
              style={appearance.mode === option.mode
                ? { backgroundColor: "var(--theme-color-surface)" }
                : undefined}
            >
              <SettingsChoiceInput
                checked={appearance.mode === option.mode}
                name="appearance-mode"
                type="radio"
                value={option.mode}
                onInput={() => {
                  appearance.setMode(option.mode)
                }}
              />
              <SettingsChoiceBody>
                <SettingsChoiceTitle>
                  {option.label}
                </SettingsChoiceTitle>
                <SettingsChoiceDescription>
                  {option.description}
                </SettingsChoiceDescription>
              </SettingsChoiceBody>
            </SettingsChoiceLabel>
          </SettingsChoiceItem>
        }
      </SettingsChoiceList>
    </SettingsFieldset>
    <SettingsFieldset>
      <SettingsLegend>"Contrast"</SettingsLegend>
      <SettingsHelperText>
        "Tightens muted text and border separation across the app."
      </SettingsHelperText>
      <SettingsControlRowLabel
        data-selected={appearance.highContrast ? "true" : undefined}
        style={appearance.highContrast
          ? { backgroundColor: "var(--theme-color-surface)" }
          : undefined}
      >
        <SettingsChoiceBody>
          <SettingsChoiceTitle>"High contrast"</SettingsChoiceTitle>
          <SettingsChoiceDescription>
            {appearance.highContrast ? "Enabled" : "Disabled"}
          </SettingsChoiceDescription>
        </SettingsChoiceBody>
        <SettingsChoiceInput
          checked={appearance.highContrast}
          type="checkbox"
          onInput={(event) => {
            appearance.setHighContrast(event.currentTarget.checked)
          }}
        />
      </SettingsControlRowLabel>
    </SettingsFieldset>
  </SettingsSection>
}
