import { useAppearance } from "~/app-state-context.tsx"
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
} from "~/settings/styled.tsx"
import type { AppearanceMode } from "./theme.ts"

const themeModeOptions = [
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
] as const satisfies {
  mode: AppearanceMode
  label: string
  description: string
}[]

export function AppearanceSettingsSection() {
  const appearance = useAppearance()

  return (
    <SettingsSection aria-labelledby="settings-appearance-heading">
      <SettingsSectionHeader>
        <SettingsSectionTitle id="settings-appearance-heading">Appearance</SettingsSectionTitle>
        <SettingsSectionDescription>
          Choose how the workbench resolves theme and contrast.
        </SettingsSectionDescription>
      </SettingsSectionHeader>

      <SettingsFieldset>
        <SettingsLegend>Color mode</SettingsLegend>
        <SettingsHelperText>
          Current effective theme: {appearance.effectiveTheme}
        </SettingsHelperText>
        <SettingsChoiceList>
          {themeModeOptions.map((option) => {
            return (
              <SettingsChoiceItem key={option.mode}>
                <SettingsChoiceLabel>
                  <SettingsChoiceInput
                    checked={appearance.mode === option.mode}
                    name="appearance-mode"
                    type="radio"
                    value={option.mode}
                    onChange={() => {
                      appearance.setMode(option.mode)
                    }}
                  />
                  <SettingsChoiceBody>
                    <SettingsChoiceTitle>{option.label}</SettingsChoiceTitle>
                    <SettingsChoiceDescription>{option.description}</SettingsChoiceDescription>
                  </SettingsChoiceBody>
                </SettingsChoiceLabel>
              </SettingsChoiceItem>
            )
          })}
        </SettingsChoiceList>
      </SettingsFieldset>

      <SettingsFieldset>
        <SettingsLegend>Contrast</SettingsLegend>
        <SettingsHelperText>
          Tightens muted text and border separation across the app.
        </SettingsHelperText>
        <SettingsControlRowLabel>
          <SettingsChoiceBody>
            <SettingsChoiceTitle>High contrast</SettingsChoiceTitle>
            <SettingsChoiceDescription>
              {appearance.highContrast ? "Enabled" : "Disabled"}
            </SettingsChoiceDescription>
          </SettingsChoiceBody>
          <SettingsChoiceInput
            checked={appearance.highContrast}
            type="checkbox"
            onChange={(event) => {
              appearance.setHighContrast(event.currentTarget.checked)
            }}
          />
        </SettingsControlRowLabel>
      </SettingsFieldset>
    </SettingsSection>
  )
}
