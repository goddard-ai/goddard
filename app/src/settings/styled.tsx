import { styled } from "@goddard-ai/styled-system/jsx";

export const SettingsSection = styled("section", {
  base: {
    display: "grid",
    gap: "20px",
    maxWidth: "720px",
  },
});

export const SettingsSectionHeader = styled("div", {
  base: {
    display: "grid",
    gap: "4px",
  },
});

export const SettingsSectionTitle = styled("h2", {
  base: {
    margin: "0",
    color: "text",
    fontSize: "1rem",
    fontWeight: "680",
    lineHeight: "1.4",
  },
});

export const SettingsSectionDescription = styled("p", {
  base: {
    margin: "0",
    color: "muted",
    fontSize: "0.88rem",
    lineHeight: "1.6",
  },
});

export const SettingsFieldset = styled("fieldset", {
  base: {
    display: "grid",
    gap: "10px",
    minWidth: "0",
    margin: "0",
    padding: "0",
    border: "0",
  },
});

export const SettingsLegend = styled("legend", {
  base: {
    padding: "0",
    color: "text",
    fontSize: "0.92rem",
    fontWeight: "620",
    lineHeight: "1.4",
  },
});

export const SettingsHelperText = styled("p", {
  base: {
    margin: "0",
    color: "muted",
    fontSize: "0.84rem",
    lineHeight: "1.55",
  },
});

export const SettingsChoiceList = styled("ul", {
  base: {
    listStyle: "none",
    margin: "0",
    padding: "0",
    borderTop: "1px solid",
    borderColor: "border",
  },
});

export const SettingsChoiceItem = styled("li", {
  base: {
    borderBottom: "1px solid",
    borderColor: "border",
  },
});

export const SettingsChoiceLabel = styled("label", {
  base: {
    display: "grid",
    gridTemplateColumns: "16px minmax(0, 1fr)",
    alignItems: "start",
    gap: "12px",
    paddingBlock: "12px",
    cursor: "pointer",
  },
});

export const SettingsChoiceInput = styled("input", {
  base: {
    width: "16px",
    height: "16px",
    margin: "0",
    marginTop: "2px",
  },
});

export const SettingsChoiceBody = styled("span", {
  base: {
    display: "grid",
    gap: "2px",
    minWidth: "0",
  },
});

export const SettingsChoiceTitle = styled("span", {
  base: {
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "620",
    lineHeight: "1.45",
  },
});

export const SettingsChoiceDescription = styled("span", {
  base: {
    color: "muted",
    fontSize: "0.84rem",
    lineHeight: "1.55",
  },
});

export const SettingsControlRowLabel = styled("label", {
  base: {
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto",
    alignItems: "start",
    gap: "12px",
    paddingBlock: "12px",
    borderTop: "1px solid",
    borderBottom: "1px solid",
    borderColor: "border",
    cursor: "pointer",
  },
});
