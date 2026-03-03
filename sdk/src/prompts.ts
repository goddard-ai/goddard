import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * Default system prompt for the autonomous agent loop (`goddard loop run`).
 *
 * Configures pi-coding-agent as a senior autonomous engineer that follows
 * Architectural Alignment, Version Stability, and Git Town workflow rules.
 * Project-level AGENTS.md files take precedence over this default.
 */
export const LOOP_SYSTEM_PROMPT: string = readFileSync(
  join(__dirname, "prompts", "loop.md"),
  "utf-8"
);

/**
 * System prompt for the spec-guardian mode (`goddard spec`).
 *
 * Configures pi-coding-agent as the Intent Guardian responsible for
 * maintaining the `spec/` Knowledge Graph. The agent translates human
 * intent into structured spec files and enforces strict anti-bloat rules.
 */
export const SPEC_SYSTEM_PROMPT: string = readFileSync(
  join(__dirname, "prompts", "spec.md"),
  "utf-8"
);
