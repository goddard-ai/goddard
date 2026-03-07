import type { SessionDriver, SessionDriverName } from "./types.ts"

const EMBEDDED_DRIVER_IMPORTS: Record<SessionDriverName, string> = {
  pi: "./pi.ts",
  gemini: "./gemini.ts",
  codex: "./codex.ts",
  pty: "./pty.ts",
}

export type DriverImporter = (specifier: string) => Promise<{ driver?: SessionDriver }>

const defaultImporter: DriverImporter = async (specifier) => {
  const module = (await import(specifier)) as { driver?: SessionDriver }
  return module
}

export async function loadEmbeddedDriver(
  name: SessionDriverName,
  importer: DriverImporter = defaultImporter,
): Promise<SessionDriver> {
  const specifier = EMBEDDED_DRIVER_IMPORTS[name]
  const loaded = await importer(specifier)

  if (!loaded.driver) {
    throw new Error(`Driver module ${specifier} did not export a driver`)
  }

  return loaded.driver
}

export function listEmbeddedDrivers(): SessionDriverName[] {
  return Object.keys(EMBEDDED_DRIVER_IMPORTS) as SessionDriverName[]
}
