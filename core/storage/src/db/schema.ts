import * as acp from "@agentclientprotocol/sdk"
import { SessionStatus } from "@goddard-ai/schema/db"
import { integer, sqliteTable, text } from "drizzle-orm/sqlite-core"

export const sessions = sqliteTable("sessions", {
  id: text().primaryKey(),
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
  serverId: text().unique(),
  serverAddress: text(),
  serverPid: integer(),
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
