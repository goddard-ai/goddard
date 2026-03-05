# `@goddard-ai/config`

Canonical home for Goddard loop configuration — schemas, types, the `Models` constant, and the `defineConfig` helper.

All exported TypeScript types are derived from Zod schemas via `z.infer<>` so the runtime validation and the static type system stay in sync automatically.

## Exports

### `Models`

Nested constant object of well-known model identifiers, organised by provider. Use it in your config file for autocomplete and typo-safety.

```typescript
import { Models } from "@goddard-ai/config";

Models.Anthropic.ClaudeSonnet45  // "anthropic/claude-sonnet-4-5"
Models.Anthropic.ClaudeSonnet46  // "anthropic/claude-sonnet-4-6"
Models.Anthropic.ClaudeOpus46    // "anthropic/claude-opus-4-6"
Models.OpenAi.O3Mini             // "openai/o3-mini"
Models.OpenAi.Gpt5Codex          // "openai/gpt-5-codex"
// …
```

### `defineConfig`

Identity helper that types your config object as `GoddardLoopConfig` and enables IDE completions.

```typescript
import { Models, defineConfig } from "@goddard-ai/config";

export default defineConfig({
  agent: {
    model: Models.Anthropic.ClaudeSonnet45,
    projectDir: "./",
    thinkingLevel: "low",
  },
  strategy: {
    nextPrompt: ({ cycleNumber, lastSummary }) =>
      `Cycle ${cycleNumber}. Last summary: ${lastSummary ?? "none"}. Make one safe improvement.`,
  },
  rateLimits: {
    cycleDelay: "30m",
    maxTokensPerCycle: 128_000,
    maxOpsPerMinute: 120,
  },
  metrics: {
    enableLogging: true,
  },
});
```

### `configSchema`

Zod schema for the full `GoddardLoopConfig`. Use it to validate a config object at runtime before passing it to `createLoop`.

```typescript
import { configSchema } from "@goddard-ai/config";

const validated = configSchema.parse(rawConfig);
```

## Exported types

| Type | Description |
|---|---|
| `GoddardLoopConfig` | Top-level loop configuration |
| `PiAgentConfig` | `agent` block — model, projectDir, thinkingLevel, … |
| `CycleStrategy` | Object with `nextPrompt(ctx: CycleContext): string` |
| `CycleContext` | Argument passed to `nextPrompt` each cycle |
| `ThinkingLevel` | `"off" \| "minimal" \| "low" \| "medium" \| "high" \| "xhigh"` |
| `Model` | Loose literal union of all known model strings (open-ended) |

## License

This project is licensed under the [MIT License](../LICENSE).
