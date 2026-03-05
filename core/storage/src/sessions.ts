import { createLocalDb } from "./db.ts";
import { piSessions } from "./schema.ts";
import { eq, sql } from "drizzle-orm";
import { mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { getLocalDbPath } from "./db.ts";

export class LocalSessionStorage {
  async ensureDb() {
    const dbPath = getLocalDbPath();
    await mkdir(dirname(dbPath), { recursive: true });

    // We create table if it doesn't exist to avoid needing a complex migration runner for the local SQLite db.
    const db = createLocalDb();

    await db.run(sql`
      CREATE TABLE IF NOT EXISTS pi_sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        repo_owner TEXT NOT NULL,
        repo_name TEXT NOT NULL,
        pr_number INTEGER NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL
      )
    `);
  }

  async createSession(owner: string, repo: string, prNumber: number) {
    await this.ensureDb();
    const db = createLocalDb();
    const createdAt = new Date().toISOString();

    const [inserted] = await db
      .insert(piSessions)
      .values({
        repoOwner: owner,
        repoName: repo,
        prNumber,
        status: "active",
        createdAt
      })
      .returning();

    return inserted;
  }

  async updateSession(id: number, status: string) {
    await this.ensureDb();
    const db = createLocalDb();

    const [updated] = await db
      .update(piSessions)
      .set({ status })
      .where(eq(piSessions.id, id))
      .returning();

    if (!updated) {
      throw new Error("Session not found");
    }

    return updated;
  }
}
