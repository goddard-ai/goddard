import * as acp from "@agentclientprotocol/sdk"
import { SessionStatus } from "@goddard-ai/schema/db"
import { integer, sqliteTable, text } from "drizzle-orm/sqlite-core"
import { v7 as uuidv7 } from "uuid"

export const sessions = sqliteTable("sessions", {
  id: text().primaryKey(),
  acpId: text().notNull().unique(),
  status: text({ enum: SessionStatus }).notNull().default("idle"),
  agentName: text().notNull(),
  cwd: text().notNull(),
  mcpServers: text({ mode: "json" }).notNull().$type<acp.McpServer[]>(),
  createdAt: integer({ mode: "timestamp" })
    .notNull()
    .$defaultFn(() => new Date()),
  updatedAt: integer({ mode: "timestamp" })
    .notNull()
    .$defaultFn(() => new Date()),
  errorMessage: text(),
  blockedReason: text(),
  initiative: text(),
  lastAgentMessage: text(),
  metadata: text({ mode: "json" }).$type<{
    repository?: string
    prNumber?: number
    [key: string]: any
  }>(),
})

export const loops = sqliteTable("loops", {
  id: text().primaryKey(),
  agent: text().notNull(),
  systemPrompt: text().notNull(),
  strategy: text(),
  displayName: text().notNull(),
  cwd: text().notNull(),
  mcpServers: text({ mode: "json" }).notNull().$type<acp.McpServer[]>(),
  gitRemote: text().notNull().default("origin"),
  createdAt: integer({ mode: "timestamp" })
    .notNull()
    .$defaultFn(() => new Date()),
  updatedAt: integer({ mode: "timestamp" })
    .notNull()
    .$defaultFn(() => new Date()),
})

export const artifacts = sqliteTable("artifacts", {
  id: text()
    .primaryKey()
    .$defaultFn(() => uuidv7()),
  sessionId: text()
    .notNull()
    .references(() => sessions.id),
  type: text().notNull(),
  metadata: text({ mode: "json" }).$type<Record<string, any>>(),
})

export const sessionPermissions = sqliteTable("session_permissions", {
  sessionId: text()
    .primaryKey()
    .references(() => sessions.id),
  token: text().notNull().unique(),
  owner: text().notNull(),
  repo: text().notNull(),
  allowedPrNumbers: text({ mode: "json" }).notNull().$type<number[]>(),
  createdAt: text().notNull(),
})
