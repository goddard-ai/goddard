import { realpath } from "node:fs/promises"
import { resolve } from "node:path"

/** Canonical daemon loop identity fields derived from one repository root and loop name. */
export interface DaemonLoopIdentity {
  rootDir: string
  loopName: string
}

/** Normalizes the repository root used to key daemon-owned loop runtimes. */
export async function normalizeLoopRootDir(rootDir: string): Promise<string> {
  return realpath(resolve(rootDir))
}

/** Builds the canonical daemon loop identity for one runtime key. */
export async function normalizeLoopIdentity(
  rootDir: string,
  loopName: string,
): Promise<DaemonLoopIdentity> {
  return {
    rootDir: await normalizeLoopRootDir(rootDir),
    loopName,
  }
}
