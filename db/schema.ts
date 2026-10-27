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

import { blob, index, integer, sqliteTable, text, uniqueIndex } from "drizzle-orm/sqlite-core";
import { sql } from "drizzle-orm";

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
  /** User-marked priority: leads attention navigation and sidebar grouping. */
  starred: integer("starred", { mode: "boolean" }).notNull().default(false),
  /** Per-project opt-in: employee `merge submit` landings stay rejected while off. */
  submissionsEnabled: integer("submissions_enabled", { mode: "boolean" }).notNull().default(false),
  /** Per-project QA branch override; NULL inherits the daemon-global qa_branch setting. */
  qaBranch: text("qa_branch"),
  /** Legacy peer marker for pre-pooling friend projects. */
  friendPeerId: text("friend_peer_id"),
  /** A non-ordinary project role — "friends" marks the pooled friend-deliveries project. */
  kind: text("kind"),
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
    /** Selected effort, carried with provider/model through catalog refreshes. */
    reasoningEffort: text("reasoning_effort"),
    status: text("status").notNull(),
    /** Session creation time, unix seconds. */
    createdAt: integer("created_at").notNull(),
    /** Any mutation, unix seconds — including title edits and truncation. */
    updatedAt: integer("updated_at").notNull(),
    /** Completion of the most recent assistant turn, unix seconds. */
    lastReplyAt: integer("last_reply_at"),
    /** When the session was archived, unix seconds; NULL while active. */
    archivedAt: integer("archived_at"),
    /** Daemon-owned CAS version and retry identity for title/archive/pin edits. */
    metadataRevision: integer("metadata_revision").notNull().default(0),
    metadataMutationId: text("metadata_mutation_id"),
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
    /**
     * The friend whose chat or transfer created this session — endpoint id
     * and display name, duplicated from `session_details.data` so sidebar
     * rows label the sender without hydrating. NULL for ordinary tasks.
     */
    friendPeerId: text("friend_peer_id"),
    friendPeerName: text("friend_peer_name"),
    /** Per-task grant for the owning agent to change this task's title. */
    agentRenameAllowed: integer("agent_rename_allowed", { mode: "boolean" }).notNull().default(false),
    /**
     * A boss chat or summoned employee task, duplicated from
     * `session_details.data` so list filtering can hide it without
     * hydrating. Unlike roster membership it survives employee retirement.
     */
    bossManaged: integer("boss_managed", { mode: "boolean" }).notNull().default(false),
    /**
     * JSON-serialized SessionPlanning, duplicated from `session_details.data`
     * so sidebar rows can badge and order planning sessions — idea, plan
     * file, freeze state — without hydrating. NULL for ordinary tasks.
     */
    planning: text("planning"),
    /**
     * JSON-serialized RuntimeEventCursor, duplicated from `session_details.data`
     * so a runtime attach can resume its replay dedup without hydrating the
     * session. NULL before the session first streams events.
     */
    runtimeEventCursor: text("runtime_event_cursor"),
    /**
     * JSON-serialized ConfigActivation, duplicated from `session_details.data`
     * so catalog projections and merges carry the confirmed activation mark
     * without hydrating the session. NULL for sessions no runtime has
     * confirmed.
     */
    configActivation: text("config_activation"),
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
    /** JSON-serialized MessageAtom array — inline atom presentation parallel
     *  to display_content's atom spans. */
    atoms: text("atoms").notNull().default("[]"),
    createdAt: integer("created_at").notNull(),
    streaming: integer("streaming", { mode: "boolean" }).notNull(),
    /** Goddard task that submitted the message through an agent credential. */
    sentByTask: text("sent_by_task"),
    /** Retired context marker column; retained for existing databases. */
    contextMark: text("context_mark"),
    /** JSON-serialized ReportTrigger — the employee report this hidden prompt
     *  delivered, so the transcript can mark which report woke the turn. */
    reportTrigger: text("report_trigger"),
    referenceContext: text("reference_context"),
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

/**
 * The daemon's durable attachment to a stable-runtime host session. One row
 * per task that ever ran through the surviving provider host: it records the
 * runtime session/generation the task is attached to and the host sequence
 * the daemon has durably applied — the cursor the runtime must never see
 * replayed or lost.
 */
export const stableRuntimeLinks = sqliteTable("stable_runtime_links", {
  /** The daemon task id this link belongs to. */
  sessionId: text("session_id").primaryKey(),
  /** The runtime namespace the owning host serves. */
  namespace: text("namespace").notNull(),
  /** The host-side provider session id (the daemon's runtime attempt id). */
  runtimeSessionId: text("runtime_session_id").notNull(),
  /** The host instance that admitted the session — a replacement is a new instance. */
  runtimeInstance: text("runtime_instance").notNull(),
  /** The host generation inside that session. */
  generation: integer("generation").notNull(),
  /** Last host event sequence durably applied to daemon state. */
  appliedSeq: integer("applied_seq").notNull().default(0),
  /** live | interrupted | ended, or applying while a daemon replay checkpoint is pending. */
  status: text("status").notNull(),
  updatedAt: integer("updated_at").notNull(),
});

/** Physical writer origins survive task deletion, link replacement and daemon restart. */
export const stableRuntimeWriters = sqliteTable("stable_runtime_writers", {
  writerKey: text("writer_key").primaryKey(),
  sessionId: text("session_id").notNull(),
  root: text("root").notNull(),
  runtimeInstance: text("runtime_instance").notNull(),
  runtimeSessionId: text("runtime_session_id").notNull(),
  generation: integer("generation").notNull(),
  workspace: text("workspace").notNull(),
  createdAt: integer("created_at").notNull(),
});

/**
 * Effect receipts for stable-runtime agent ingress. The request identity is
 * the durable key: a forwarded request whose answer the daemon already
 * recorded is answered from the receipt, never executed twice — and an
 * outcome that could not be confirmed stays `uncertain`, never resent.
 */
export const stableRuntimeReceipts = sqliteTable("stable_runtime_receipts", {
  /** The runtime's request identity — also the dedup key for effects. */
  requestId: text("request_id").primaryKey(),
  /** The daemon task the request's grant was scoped to. */
  sessionId: text("session_id").notNull(),
  generation: integer("generation").notNull(),
  /** cli | mcp | callback — which agent surface forwarded it. */
  class: text("class").notNull(),
  /** JSON outcome recorded for the request before delivery was confirmed. */
  outcome: text("outcome").notNull(),
  recordedAt: integer("recorded_at").notNull(),
});

/** Explicit provider-stop authority survives daemon and attachment replacement. */
export const stableRuntimeStops = sqliteTable(
  "stable_runtime_stops",
  {
    commandId: text("command_id").primaryKey(),
    sessionId: text("session_id").notNull(),
    namespace: text("namespace").notNull(),
    root: text("root").notNull(),
    runtimeInstance: text("runtime_instance").notNull(),
    runtimeSessionId: text("runtime_session_id").notNull(),
    generation: integer("generation").notNull(),
    requestedAt: integer("requested_at").notNull(),
    /** Owner JSON without a secret; JSON null is known absent, SQL NULL is legacy unknown. */
    descriptorOwner: text("descriptor_owner"),
    /** Set only after the exact host generation confirms termination. */
    confirmedAt: integer("confirmed_at"),
    retiredAt: integer("retired_at"),
    retryAt: integer("retry_at").notNull().default(0),
    failures: integer("failures").notNull().default(0),
    lastError: text("last_error"),
  },
  (table) => [
    uniqueIndex("stable_runtime_stop_owner").on(
      table.runtimeInstance, table.runtimeSessionId, table.generation,
    ),
    index("stable_runtime_stop_ready").on(table.retiredAt, table.retryAt, table.commandId),
  ],
);

/** Durable archive-workspace intent. A row records work owed, never permission to delete. */
export const workspaceCleanupIntents = sqliteTable(
  "workspace_cleanup_intents",
  {
    sessionId: text("session_id").primaryKey(),
    /** New opaque identity for each archive/workspace transition. */
    generation: text("generation").notNull(),
    projectId: text("project_id").notNull(),
    projectPath: text("project_path"),
    /** Exact promoted workspace descriptor; NULL means the project's ordinary cwd. */
    workspace: text("workspace"),
    archivedAt: integer("archived_at").notNull(),
    /** pending | captured | verified | retired | complete | ignored */
    phase: text("phase").notNull().default("pending"),
    retryAt: integer("retry_at").notNull().default(0),
    failures: integer("failures").notNull().default(0),
    lastError: text("last_error"),
  },
  (table) => [index("workspace_cleanup_ready").on(table.phase, table.retryAt)],
);

/** Unqualified terminal origins survive closure and restart; no task FK or exit inference. */
export const terminalWorkspaceWriters = sqliteTable(
  "terminal_workspace_writers",
  {
    workspace: text("workspace").notNull(),
    /** Directory identity at spawn; reopening a reused path must retain the older origin. */
    identity: text("identity").notNull(),
    createdAt: integer("created_at").notNull(),
  },
  (table) => [uniqueIndex("terminal_workspace_writer_origin").on(table.workspace, table.identity)],
);

/** Native attempt identity survives runtime/task removal; only its qualified witness confirms it. */
export const nativeWorkspaceWriters = sqliteTable(
  "native_workspace_writers",
  {
    originId: text("origin_id").primaryKey(),
    sessionId: text("session_id").notNull(),
    runtimeId: text("runtime_id").notNull(),
    workspace: text("workspace").notNull(),
    /** Null means this platform could not qualify the directory identity. */
    identity: text("identity"),
    confirmed: integer("confirmed").notNull().default(0),
    createdAt: integer("created_at").notNull(),
  },
  (table) => [index("native_unconfirmed_writers").on(table.originId)
    .where(sql`${table.confirmed} is not 1`)],
);
