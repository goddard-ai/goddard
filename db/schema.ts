/**
 * Goddard local state schema.
 *
 * Drizzle is a build-time tool here: `bun run db:generate` diffs this file and
 * writes plain SQL into `db/migrations`, which the Rust app applies at startup
 * (see `apply_migrations` in `src/persistence.rs`). drizzle-orm never ships in
 * the binary — Rust owns every query.
 *
 * Session history is kept out of the `sessions` row: the row holds only what
 * the session list renders, so listing is a scan over narrow rows. The
 * transcript lives in `session_details` and messages in `messages`, both
 * fetched only when a session is opened.
 */

import { blob, index, integer, sqliteTable, text } from "drizzle-orm/sqlite-core";

export const projects = sqliteTable("projects", {
  id: text("id").primaryKey(),
  name: text("name").notNull(),
  path: text("path").notNull(),
  /** Finder-bookmark data that re-resolves the folder after a rename. */
  bookmark: blob("bookmark"),
  /** Order shown in the sidebar. */
  position: integer("position").notNull(),
  /** When the project was added, unix seconds. */
  createdAt: integer("created_at").notNull(),
  /** Picked ad hoc for a task rather than registered; swept with its last session. */
  temporary: integer("temporary", { mode: "boolean" }).notNull().default(false),
});

export const sessions = sqliteTable(
  "sessions",
  {
    id: text("id").primaryKey(),
    projectId: text("project_id").notNull(),
    /** Explicit user title; "New task" means the automatic fallback is active. */
    title: text("title").notNull(),
    /** Provider-generated title, with the first prompt as a local fallback. */
    autoTitle: text("auto_title"),
    provider: text("provider").notNull(),
    model: text("model"),
    status: text("status").notNull(),
    /** Session creation time, unix seconds. */
    createdAt: integer("created_at").notNull(),
    /** Any mutation, unix seconds — including title edits and truncation. */
    updatedAt: integer("updated_at").notNull(),
    /** Completion of the most recent assistant turn, unix seconds. */
    lastReplyAt: integer("last_reply_at"),
    /** When the session was archived, unix seconds; NULL while active. */
    archivedAt: integer("archived_at"),
    /** When the session was pinned to the sidebar top, unix seconds. */
    pinnedAt: integer("pinned_at"),
    /** When the session was swept into the Dormant group, unix seconds. */
    dormantAt: integer("dormant_at"),
    /** Auto-dormancy is suppressed until this time, unix seconds. */
    dormantExemptUntil: integer("dormant_exempt_until"),
    /** When the session's workspace landed on its base, unix seconds. */
    landedAt: integer("landed_at"),
    /**
     * JSON-serialized SessionWorkspace, duplicated from `session_details.data`
     * so list rows can show worktree state without hydrating the session.
     * NULL while the session works in the project's ordinary checkout.
     */
    workspace: text("workspace"),
    /**
     * The parent task this session is a side chat of; NULL for ordinary
     * tasks. Side chats stay out of task lists and die with their parent.
     */
    sideChatOf: text("side_chat_of"),
  },
  (table) => [
    index("sessions_by_project").on(table.projectId, table.updatedAt),
    index("sessions_by_updated_at").on(table.updatedAt),
    index("sessions_by_last_reply_at").on(table.lastReplyAt),
  ],
);

/**
 * Conversation messages, one row each.
 *
 * Split out of `sessions.data` so appending to a long conversation writes one
 * small row instead of rewriting the whole history, and so a message can be
 * read or counted without deserializing a transcript.
 */
export const messages = sqliteTable(
  "messages",
  {
    id: text("id").primaryKey(),
    sessionId: text("session_id").notNull(),
    turnId: text("turn_id"),
    /** Ordinal within the session; conversation order, not wall-clock. */
    position: integer("position").notNull(),
    role: text("role").notNull(),
    content: text("content").notNull(),
    /** User-visible text before provider-facing attachment mentions. */
    displayContent: text("display_content"),
    /** JSON-serialized TranscriptNotice — a structured system row's payload. */
    notice: text("notice"),
    /** JSON-serialized MessageAttachment array. */
    attachments: text("attachments").notNull().default("[]"),
    createdAt: integer("created_at").notNull(),
    streaming: integer("streaming", { mode: "boolean" }).notNull(),
    /** Goddard task that submitted the message through an agent credential. */
    sentByTask: text("sent_by_task"),
    /** Provider-facing text no client renders — the internal "continue" nudge. */
    hidden: integer("hidden", { mode: "boolean" }).notNull().default(false),
  },
  (table) => [index("messages_by_session").on(table.sessionId, table.position)],
);

/**
 * The rest of `AgentSession` as JSON — transcript blocks, turns, provider
 * cursor.
 *
 * Split from `sessions` because it is large and rarely read: keeping it in the
 * row would mean listing sessions pages through every transcript, and every
 * title edit rewrites a transcript-sized row.
 */
export const sessionDetails = sqliteTable("session_details", {
  sessionId: text("session_id").primaryKey(),
  data: text("data").notNull(),
});
