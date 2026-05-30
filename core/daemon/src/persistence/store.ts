import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { getDatabasePath } from "@goddard-ai/paths/node"
import {
  kindstore,
  UnrecoverableStoreOpenError,
  type DatabaseOptions,
  type KindRegistry,
} from "kindstore"
import { z } from "zod"

type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

const metadata = {
  authToken: z.string(),
}

const coreDbSchema = {}

/** Runtime daemon store handle opened against the composed plugin schema. */
export type DaemonStore = any

let activeSchema: KindRegistry = coreDbSchema
let activeConnection: StoreConnectionOptions = { filename: getDatabasePath() }

/** Opens one kindstore handle for a concrete daemon store schema. */
function openKindstore<const TSchema extends KindRegistry>(
  options: StoreConnectionOptions & { schema: keyof TSchema extends never ? never : TSchema },
) {
  if (options.filename !== ":memory:") {
    mkdirSync(dirname(options.filename), { recursive: true })
  }

  return kindstore({
    filename: options.filename,
    databaseOptions: options.databaseOptions,
    metadata,
    schema: options.schema,
  })
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

/** Opens one daemon store connection against the currently configured plugin schema. */
export function openDaemonStore(
  connection: StoreConnectionOptions = activeConnection,
): DaemonStore {
  try {
    return openKindstore({ ...connection, schema: activeSchema }) as DaemonStore
  } catch (error) {
    if (connection.filename === ":memory:" || !(error instanceof UnrecoverableStoreOpenError)) {
      throw error
    }

    removeDatabaseArtifacts(connection.filename)
    return openKindstore({ ...connection, schema: activeSchema }) as DaemonStore
  }
}

/** Sets the feature-contributed store schema before daemon store handles are opened. */
export function configureDbSchema(pluginSchema: KindRegistry) {
  activeSchema = mergeDbSchema(pluginSchema)
}

function mergeDbSchema(pluginSchema: KindRegistry) {
  const schema: KindRegistry = { ...coreDbSchema }

  for (const [key, kindDefinition] of Object.entries(pluginSchema)) {
    if (Object.hasOwn(coreDbSchema, key)) {
      throw new Error(`Daemon plugin DB collection conflicts with core store schema: ${key}`)
    }
    schema[key] = kindDefinition
  }

  return schema
}

/** Opens a fresh daemon store handle for tests, optionally with explicit connection options. */
export function resetDb(connection: StoreConnectionOptions = { filename: getDatabasePath() }) {
  activeConnection = connection
  return openDaemonStore(connection)
}
