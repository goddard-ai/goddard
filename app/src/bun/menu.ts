import { ApplicationMenu } from "electrobun/bun"

/** Installs the standard macOS Edit menu so native text shortcuts work in webviews. */
export function installMacOsApplicationMenu(): void {
  if (process.platform !== "darwin") {
    return
  }

  ApplicationMenu.setApplicationMenu([
    {
      submenu: [{ label: "Quit", role: "quit" }],
    },
    {
      label: "Edit",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "pasteAndMatchStyle" },
        { role: "delete" },
        { role: "selectAll" },
      ],
    },
  ])
}
