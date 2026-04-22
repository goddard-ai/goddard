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

  <tsx:react>
    <SettingsSection aria-labelledby="settings-appearance-heading">
      <SettingsSectionHeader>
        <SettingsSectionTitle id="settings-appearance-heading">Appearance</SettingsSectionTitle>
        <SettingsSectionDescription>Choose how the workbench resolves theme and contrast.</SettingsSectionDescription>
      </SettingsSectionHeader>
      <SettingsFieldset>
        <SettingsLegend>Color mode</SettingsLegend>
        <SettingsHelperText>
          Current effective theme:
          {appearance.effectiveTheme}
        </SettingsHelperText>
        <SettingsChoiceList>
          {themeModeOptions.map((option) => {
            return <tsx:react>
              <SettingsChoiceItem key={option.mode}>
                <SettingsChoiceLabel>
                  <SettingsChoiceInput checked={appearance.mode ===
                    option.mode} name="appearance-mode" type="radio" value={option.mode} onInput={() => {
                    appearance.setMode(option.mode)
                  }} />
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
            </tsx:react>
          })}
        </SettingsChoiceList>
      </SettingsFieldset>
      <SettingsFieldset>
        <SettingsLegend>Contrast</SettingsLegend>
        <SettingsHelperText>Tightens muted text and border separation across the app.</SettingsHelperText>
        <SettingsControlRowLabel>
          <SettingsChoiceBody>
            <SettingsChoiceTitle>High contrast</SettingsChoiceTitle>
            <SettingsChoiceDescription>
              {appearance.highContrast ? "Enabled" : "Disabled"}
            </SettingsChoiceDescription>
          </SettingsChoiceBody>
          <SettingsChoiceInput checked={appearance.highContrast} type="checkbox" onInput={(
            event,
          ) => {
            appearance.setHighContrast(event.currentTarget.checked)
          }} />
        </SettingsControlRowLabel>
      </SettingsFieldset>
    </SettingsSection>
  </tsx:react>
}
