import { realpath } from "node:fs/promises"
import { join, resolve } from "node:path"

/** Canonical filesystem paths used by one daemon-managed workforce runtime. */
export interface WorkforcePaths {
  rootDir: string
  goddardDir: string
  configPath: string
  ledgerPath: string
}

export async function normalizeWorkforceRootDir(rootDir: string): Promise<string> {
  return realpath(resolve(rootDir))
}

export function buildWorkforcePaths(rootDir: string): WorkforcePaths {
  const goddardDir = join(rootDir, ".goddard")

  return {
    rootDir,
    goddardDir,
    configPath: join(goddardDir, "workforce.json"),
    ledgerPath: join(goddardDir, "ledger.jsonl"),
  }
}
