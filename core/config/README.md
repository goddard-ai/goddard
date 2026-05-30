# `@goddard-ai/config`

Shared helpers for Goddard config resolution.

Persisted config is JSON-only:

- Global defaults: `~/.goddard/config.json`
- Local defaults: `<repo>/.goddard/config.json`
- Packaged action defaults: `.goddard/actions/<name>/config.json`
- Packaged loop defaults: `.goddard/loops/<name>/config.json`

## Exports

Use the generic helpers to apply deterministic precedence inside config owners:

```ts
import { mergeConfigLayers, selectLast } from "@goddard-ai/config"

const merged = mergeConfigLayers([globalConfig, localConfig])
const session = selectLast([globalConfig, localConfig], (config) => config?.session)
```

## Notes

- Root config validation is daemon-owned because the effective schema is derived
  from the statically composed daemon plugin list.
- Feature packages own their feature-specific schemas and merge behavior.
- Persisted loop config must remain JSON-safe. `nextPrompt` is not stored in JSON.
- Runnable loop packages load `nextPrompt` from `prompt.js`.
- Prompt frontmatter is not a supported config surface.

## License

This project is licensed under the [Functional Source License, Version 1.1, ALv2 Future License](./LICENSE).
