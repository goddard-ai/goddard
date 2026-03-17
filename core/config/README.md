# `@goddard-ai/config`

Canonical home for Goddard's persisted JSON configuration schemas, merge helpers, and shared model constants.

Persisted config is JSON-only:

- Global defaults: `~/.goddard/config.json`
- Local defaults: `<repo>/.goddard/config.json`
- Packaged action defaults: `.goddard/actions/<name>/config.json`
- Packaged loop defaults: `.goddard/loops/<name>/config.json`

## Exports

### `Models`

Provider-grouped model identifiers for runtime or persisted config payloads.

```ts
import { Models } from "@goddard-ai/config"

Models.Anthropic.ClaudeSonnet45
Models.OpenAi.Gpt53Codex
```

### Persisted document schemas

Use these to validate JSON documents before loading them:

```ts
import {
  actionConfigSchema,
  loopConfigSchema,
  rootConfigSchema,
} from "@goddard-ai/config"

const rootConfig = rootConfigSchema.parse(rawRootConfig)
const actionConfig = actionConfigSchema.parse(rawActionConfig)
const loopConfig = loopConfigSchema.parse(rawLoopConfig)
```

### Merge helpers

Use these to apply deterministic `global -> local -> entity -> runtime` precedence:

```ts
import {
  mergeActionConfigLayers,
  mergeLoopConfigLayers,
  mergeRootConfigLayers,
} from "@goddard-ai/config"

const rootConfig = mergeRootConfigLayers(globalRoot, localRoot)
const actionConfig = mergeActionConfigLayers(rootConfig.actions, entityConfig, runtimeOverrides)
const loopConfig = mergeLoopConfigLayers(rootConfig.loops, entityConfig, runtimeOverrides)
```

## Exported types

| Type | Description |
| --- | --- |
| `GoddardRootConfigDocument` | Shared JSON document for root action and loop defaults |
| `GoddardActionConfigDocument` | JSON-safe action defaults layered before runtime overrides |
| `GoddardLoopConfigDocument` | JSON-safe loop defaults layered before runtime overrides |
| `ResolvedGoddardLoopConfigDocument` | Fully required loop config after defaults and validation |
| `ThinkingLevel` | `"off" \| "minimal" \| "low" \| "medium" \| "high" \| "xhigh"` |
| `Model` | Loose literal union of known model strings plus custom values |

## Notes

- Persisted loop config must remain JSON-safe. `nextPrompt` is not stored in JSON.
- Runnable loop packages load `nextPrompt` from `prompt.js`.
- Prompt frontmatter is not a supported config surface.

## License

This project is licensed under the [MIT License](../LICENSE).
