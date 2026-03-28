# `@goddard-ai/config`

Canonical home for Goddard's persisted-config merge helpers.

Persisted config is JSON-only:

- Global defaults: `~/.goddard/config.json`
- Local defaults: `<repo>/.goddard/config.json`
- Packaged action defaults: `.goddard/actions/<name>/config.json`
- Packaged loop defaults: `.goddard/loops/<name>/config.json`

## Exports

### Persisted document schemas

Use these to validate JSON documents before loading them:

```ts
import {
  ActionConfig,
  LoopConfig,
  RootConfig,
} from "@goddard-ai/schema/config"

const rootConfig = RootConfig.parse(rawRootConfig)
const actionConfig = ActionConfig.parse(rawActionConfig)
const loopConfig = LoopConfig.parse(rawLoopConfig)
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

## Notes

- Persisted loop config must remain JSON-safe. `nextPrompt` is not stored in JSON.
- Runnable loop packages load `nextPrompt` from `prompt.js`.
- Prompt frontmatter is not a supported config surface.

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
