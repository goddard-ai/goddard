import { Updater } from "electrobun/bun"

/** Result state for one host-owned app update check. */
type UpdateCheckResult = "disabled" | "current" | "ready" | "unavailable"

let activeUpdateCheck: Promise<UpdateCheckResult> | undefined

/** Checks for a packaged app update and downloads it when one is available. */
export function checkAndDownloadUpdate() {
  activeUpdateCheck ??= (async () => {
    if (Updater.updateInfo()?.updateReady) {
      return "ready"
    }

    if ((await Updater.localInfo.channel()) === "dev") {
      return "disabled"
    }

    const info = await Updater.checkForUpdate()
    if (!info.updateAvailable) {
      if (info.error) {
        console.error("Update check failed.", info.error)
        return "unavailable"
      }

      return "current"
    }

    await Updater.downloadUpdate()
    const downloadedInfo = Updater.updateInfo()

    if (downloadedInfo?.updateReady) {
      console.log(`Update ${downloadedInfo.version} is ready to install.`)
      return "ready"
    }

    if (downloadedInfo?.error) {
      console.error("Update download failed.", downloadedInfo.error)
    }

    return "unavailable"
  })().finally(() => {
    activeUpdateCheck = undefined
  })
  return activeUpdateCheck
}

/** Starts one non-blocking update check after the desktop shell is open. */
export function startBackgroundUpdateCheck() {
  void checkAndDownloadUpdate().catch((error) => {
    console.error("Background update check failed.", error)
  })
}

/** Applies an already downloaded update, downloading it first when needed. */
export async function applyReadyUpdate() {
  if (Updater.updateInfo()?.updateReady) {
    await Updater.applyUpdate()
    return "ready"
  }

  const result = await checkAndDownloadUpdate()

  if (result !== "ready") {
    return result
  }

  await Updater.applyUpdate()
  return "ready"
}
