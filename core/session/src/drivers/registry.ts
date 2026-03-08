import type { SessionDriver, SessionDriverName } from "./types.ts"

type SessionDriverConstructor = new () => SessionDriver
type DriverImportThunk = () => Promise<{ default?: SessionDriverConstructor }>

const EMBEDDED_DRIVER_IMPORTS: Record<SessionDriverName, DriverImportThunk> = {
  pi: () => import("./pi.ts"),
  gemini: () => import("./gemini.ts"),
  codex: () => import("./codex.ts"),
  pty: () => import("./pty.ts"),
}

export type DriverImporter = (
  name: SessionDriverName,
) => Promise<{ default?: SessionDriverConstructor }>

const defaultImporter: DriverImporter = async (name) => {
  const importThunk = EMBEDDED_DRIVER_IMPORTS[name]
  if (!importThunk) {
    throw new Error(`Driver module for ${name} not found`)
  }
  return await importThunk()
}

export async function loadEmbeddedDriver(
  name: SessionDriverName,
  importer: DriverImporter = defaultImporter,
): Promise<SessionDriver> {
  const loaded = await importer(name)

  if (!loaded.default) {
    throw new Error(`Driver module for ${name} did not export a default driver class`)
  }

  return new loaded.default()
}

export function listEmbeddedDrivers(): SessionDriverName[] {
  return Object.keys(EMBEDDED_DRIVER_IMPORTS) as SessionDriverName[]
}
