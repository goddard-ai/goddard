-- Authoritative schema managed by sqlite3def.
--
-- Apply with:
--   sqlite3def --file backend/schema.sql <path-to-db-file>
--
-- sqlite3def diffs this desired state against the live database and emits
-- only the ALTER TABLE / CREATE TABLE DDL required to converge — no
-- hand-written migrations.
--
-- Editing rules:
--   • No IF NOT EXISTS — sqlite3def owns idempotency.
--   • No PRAGMA statements — put those in init.sql.
--   • No INSERT / seed data — put those in init.sql.
--   • No DEFAULT (function()) expressions — sqlite3def cannot parse function
--     calls inside DEFAULT clauses (e.g. DEFAULT (datetime('now'))). Columns
--     that need a server-side timestamp default should use a trigger defined
--     in init.sql; columns whose values are always set by the application
--     (as all created_at fields here are, via Drizzle) need no DB default.

CREATE TABLE users (
  github_user_id INTEGER PRIMARY KEY,
  github_username TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE auth_sessions (
  token TEXT PRIMARY KEY,
  github_user_id INTEGER NOT NULL REFERENCES users(github_user_id),
  github_username TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE pull_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  number INTEGER NOT NULL,
  owner TEXT NOT NULL,
  repo TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT,
  head TEXT NOT NULL,
  base TEXT NOT NULL,
  url TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
