import { sqliteTable, text, integer } from "drizzle-orm/sqlite-core"

export const users = sqliteTable("users", {
  githubUserId: integer("github_user_id").primaryKey(),
  githubUsername: text("github_username").notNull(),
  createdAt: text("created_at").notNull(),
})

export const authSessions = sqliteTable("auth_sessions", {
  token: text("token").primaryKey(),
  githubUserId: integer("github_user_id")
    .notNull()
    .references(() => users.githubUserId),
  githubUsername: text("github_username").notNull(),
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

export const repoEvents = sqliteTable("repo_events", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  githubUsername: text("github_username").notNull(),
  eventJson: text("event_json").notNull(),
  createdAt: text("created_at").notNull(),
})
