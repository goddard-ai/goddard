import { integer, primaryKey, sqliteTable, text } from "drizzle-orm/sqlite-core"

export const users = sqliteTable("users", {
  id: text("id").primaryKey(),
  createdAt: text("created_at").notNull(),
})

export const providerIdentities = sqliteTable(
  "provider_identities",
  {
    provider: text("provider").notNull(),
    subject: text("subject").notNull(),
    principalId: text("principal_id")
      .notNull()
      .references(() => users.id),
    displayName: text("display_name"),
    createdAt: text("created_at").notNull(),
  },
  (table) => [primaryKey({ columns: [table.provider, table.subject] })],
)

export const authSessions = sqliteTable("auth_sessions", {
  token: text("token").primaryKey(),
  principalId: text("principal_id")
    .notNull()
    .references(() => users.id),
  expiresAt: integer("expires_at").notNull(),
  createdAt: text("created_at").notNull(),
})

export const pullRequests = sqliteTable("pull_requests", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  number: integer("number").notNull(),
  owner: text("owner").notNull(),
  repo: text("repo").notNull(),
  title: text("title").notNull(),
  body: text("body"),
  head: text("head").notNull(),
  base: text("base").notNull(),
  url: text("url").notNull(),
  createdBy: text("created_by").notNull(),
  createdAt: text("created_at").notNull(),
})
