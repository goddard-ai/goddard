import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

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

/**
 * System prompt for the feature proposer mode (`goddard propose`).
 *
 * Configures pi-coding-agent as the Feature Proposer responsible for
 * drafting structured feature proposals in the `spec/proposals/` directory.
 */
export const PROPOSE_SYSTEM_PROMPT: string = readFileSync(
  join(__dirname, "prompts", "propose.md"),
  "utf-8"
);
