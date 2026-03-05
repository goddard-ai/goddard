import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * Default system prompt for the autonomous agent loop (`goddard loop run`).
 *
 * Configures pi-coding-agent as a senior autonomous engineer that follows
 * Architectural Alignment, Version Stability, and standard workflow rules.
 * Project-level AGENTS.md files take precedence over this default.
 */
export const LOOP_SYSTEM_PROMPT: string = readFileSync(
  join(__dirname, "prompts", "loop.md"),
  "utf-8"
);
