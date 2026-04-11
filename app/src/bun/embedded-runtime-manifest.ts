/** Folder name used for the app-bundled daemon runtime payload inside Electrobun resources. */
export const embeddedRuntimeDirName = "embedded-runtime"

/** Stable user-scoped service name used when registering the desktop-managed daemon. */
export const daemonServiceName = "goddard-daemon"

/** Pinned upstream serviceman version staged into the desktop app bundle. */
export const embeddedServicemanVersion = "v0.9.5"

/** App-bundled daemon runtime manifest written during the Electrobun prebuild step. */
export type EmbeddedRuntimeManifest = {
  formatVersion: 1
  target: {
    os: "macos" | "linux" | "win"
    arch: "arm64" | "x64"
    bunTarget: string
  }
  daemon: {
    version: string
    runtimeHash: string
    executablePath: string
    agentBinDir: string
    helperPaths: {
      goddard: string
      workforce: string
    }
  }
  serviceman: {
    version: string
    launcherPath: string
    shareDir: string
  }
}
