import { mkdirSync, rmSync } from "node:fs"
import { dirname } from "node:path"
import { getDatabasePath } from "@goddard-ai/paths/node"
import {
  kindstore,
  UnrecoverableStoreOpenError,
  type DatabaseOptions,
  type KindBuilder,
  type KindRegistry,
  type Kindstore,
  type SchemaMigrationPlanner,
} from "kindstore"
import { z } from "zod"

export type StoreConnectionOptions = {
  filename: string
  databaseOptions?: DatabaseOptions
}

export type StoreRecoveryHandler = (input: {
  filename: string
  error: UnrecoverableStoreOpenError
}) => void

const metadata = {
  authToken: z.string(),
  browserAccess: z.strictObject({
    pendingPairings: z.record(
      z.string(),
      z.strictObject({
        origin: z.string(),
        codeHash: z.string(),
        label: z.string().nullable(),
        createdAt: z.string(),
        expiresAt: z.string(),
        confirmedAt: z.string().nullable(),
        completedAt: z.string().nullable(),
      }),
    ),
    browserTokens: z.record(
      z.string(),
      z.strictObject({
        origin: z.string(),
        tokenHash: z.string(),
        label: z.string().nullable(),
        createdAt: z.string(),
        revokedAt: z.string().nullable(),
      }),
    ),
  }),
  managedAgentUpdateChecks: z.record(
    z.string(),
    z.strictObject({
      checkedAt: z.string(),
      configFingerprint: z.string(),
    }),
  ),
  managedAgentUsage: z.record(
    z.string(),
    z.strictObject({
      lastUsedAt: z.string(),
    }),
  ),
}

const coreDbSchema = {} satisfies KindRegistry

type DaemonDbDefinition<TSchema extends KindRegistry> = {
  readonly schema: TSchema
  readonly migrate?: (planner: SchemaMigrationPlanner<InferKinds<TSchema>>) => void
}

/** Daemon store handle opened against a concrete plugin-contributed schema. */
export type DaemonStore<TSchema extends KindRegistry = KindRegistry> = Kindstore<
  InferKinds<TSchema>,
  typeof metadata
>

type InferKinds<TSchema extends KindRegistry> = {
  readonly [K in keyof TSchema as TSchema[K] extends KindBuilder<any> ? K : never]: Extract<
    TSchema[K],
    KindBuilder<any>
  >
}
type NonEmptySchema<TSchema extends KindRegistry> = keyof TSchema extends never ? never : TSchema

/** Opens one kindstore handle for a concrete daemon store schema. */
function openKindstore<const TSchema extends KindRegistry>(
  options: StoreConnectionOptions & DaemonDbDefinition<TSchema>,
): DaemonStore<TSchema> {
  if (options.filename !== ":memory:") {
    mkdirSync(dirname(options.filename), { recursive: true })
  }

  return kindstore({
    filename: options.filename,
    databaseOptions: options.databaseOptions,
    metadata,
    schema: options.schema as NonEmptySchema<TSchema>,
    migrate: options.migrate,
  }) as unknown as DaemonStore<TSchema>
}

function removeDatabaseArtifacts(filename: string) {
  for (const suffix of ["", "-shm", "-wal"]) {
    rmSync(`${filename}${suffix}`, { force: true })
  }
}

/** Opens one daemon store connection against a concrete plugin-contributed schema. */
export function openDaemonStore<const TSchema extends KindRegistry>(
  pluginDb: DaemonDbDefinition<TSchema>,
  connection: StoreConnectionOptions = { filename: getDatabasePath() },
  onRecovery?: StoreRecoveryHandler,
): DaemonStore<TSchema> {
  const schema = mergeDbSchema(pluginDb.schema)
  const db = { schema, migrate: pluginDb.migrate }

  try {
    return openKindstore({ ...connection, ...db })
  } catch (error) {
    if (connection.filename === ":memory:" || !(error instanceof UnrecoverableStoreOpenError)) {
      throw error
    }

    try {
      onRecovery?.({ filename: connection.filename, error })
    } catch {
      // Recovery must remain available when its observability callback fails.
    }
    removeDatabaseArtifacts(connection.filename)
    return openKindstore({ ...connection, ...db })
  }
}

function mergeDbSchema<const TSchema extends KindRegistry>(pluginSchema: TSchema) {
  const schema = { ...coreDbSchema } as typeof coreDbSchema & TSchema

  for (const [key, kindDefinition] of Object.entries(pluginSchema)) {
    if (Object.hasOwn(coreDbSchema, key)) {
      throw new Error(`Daemon plugin DB collection conflicts with core store schema: ${key}`)
    }
    schema[key as keyof TSchema] = kindDefinition as TSchema[keyof TSchema]
  }

  return schema
}
