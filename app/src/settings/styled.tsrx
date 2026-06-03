import { styled } from "@goddard-ai/styled-system/jsx"

export const SettingsSection = styled("section", {
  base: {
    display: "grid",
    gap: "0",
    maxWidth: "760px",
    overflow: "hidden",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "panel",
  },
})

export const SettingsSectionHeader = styled("div", {
  base: {
    display: "grid",
    gap: "5px",
    padding: "18px 20px 16px",
  },
})

export const SettingsSectionTitle = styled("h2", {
  base: {
    margin: "0",
    color: "text",
    fontSize: "1.05rem",
    fontWeight: "720",
    lineHeight: "1.4",
  },
})

export const SettingsSectionDescription = styled("p", {
  base: {
    margin: "0",
    color: "muted",
    fontSize: "0.86rem",
    lineHeight: "1.6",
  },
})

export const SettingsFieldset = styled("fieldset", {
  base: {
    display: "grid",
    gap: "10px",
    minWidth: "0",
    margin: "0",
    padding: "18px 20px",
    border: "0",
    borderTop: "1px solid {colors.border}",
  },
})

export const SettingsLegend = styled("legend", {
  base: {
    padding: "0",
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "680",
    lineHeight: "1.4",
  },
})

export const SettingsHelperText = styled("p", {
  base: {
    margin: "0",
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.55",
  },
})

export const SettingsChoiceList = styled("ul", {
  base: {
    listStyle: "none",
    margin: "0",
    padding: "0",
    display: "grid",
    gap: "8px",
  },
})

export const SettingsChoiceItem = styled("li", {
  base: {
    minWidth: "0",
  },
})

export const SettingsChoiceLabel = styled("label", {
  base: {
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) 22px",
    alignItems: "center",
    gap: "12px",
    minHeight: "64px",
    padding: "12px 12px 12px 14px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "background",
    cursor: "pointer",
  },
})

export const SettingsChoiceInput = styled("input", {
  base: {
    appearance: "auto",
    width: "16px",
    height: "16px",
    margin: "0",
    accentColor: "accentStrong",
    cursor: "pointer",
    _focusVisible: {
      outline: "2px solid {colors.accentStrong}",
      outlineOffset: "2px",
    },
  },
})

export const SettingsChoiceBody = styled("span", {
  base: {
    display: "grid",
    gap: "3px",
    minWidth: "0",
  },
})

export const SettingsChoiceTitle = styled("span", {
  base: {
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "660",
    lineHeight: "1.45",
  },
})

export const SettingsChoiceDescription = styled("span", {
  base: {
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.55",
  },
})

export const SettingsControlRowLabel = styled("label", {
  base: {
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto",
    alignItems: "center",
    gap: "12px",
    minHeight: "64px",
    padding: "12px 14px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "background",
    cursor: "pointer",
  },
})
