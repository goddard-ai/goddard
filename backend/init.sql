-- Bootstrap script — always run before sqlite3def.
--
-- This file handles items sqlite3def cannot manage. It is not a fallback;
-- it is a required first step in every environment, applied via:
--
--   sqlite3 <db-file> < backend/init.sql
--   sqlite3def --file backend/schema.sql <db-file>
--
-- What belongs here:
--
--   1. PRAGMA statements — sqlite3def never emits these. PRAGMA foreign_keys
--      in particular must be set or SQLite silently ignores REFERENCES
--      constraints at runtime.
--
--   2. Seed / bootstrap data — INSERT statements that must exist before the
--      application can start (e.g. default roles, system config rows).
--
--   3. Triggers and views — sqlite3def has no support for CREATE TRIGGER or
--      CREATE VIEW; define them here.
--
--   4. Function-based column defaults — sqlite3def cannot parse DEFAULT
--      (expr()) clauses. If a column needs a server-side default computed by
--      a function (e.g. datetime('now')), implement it as a BEFORE INSERT
--      trigger here rather than a column default in schema.sql.

-- ---------------------------------------------------------------------------
-- 1. Pragmas
-- ---------------------------------------------------------------------------

-- SQLite does not enforce foreign-key constraints by default.
-- This must be re-applied every time a connection opens; sqlite3def never
-- emits it.
PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- 2. Seed data  (add INSERT statements here as the project requires)
-- ---------------------------------------------------------------------------

-- ---------------------------------------------------------------------------
-- 3. Triggers / views  (add CREATE TRIGGER / CREATE VIEW here)
-- ---------------------------------------------------------------------------
