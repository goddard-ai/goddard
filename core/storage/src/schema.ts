import { sqliteTable, integer, text } from "drizzle-orm/sqlite-core";

export const piSessions = sqliteTable("pi_sessions", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  repoOwner: text("repo_owner").notNull(),
  repoName: text("repo_name").notNull(),
  prNumber: integer("pr_number").notNull(),
  status: text("status").notNull(),
  createdAt: text("created_at").notNull(),
});
