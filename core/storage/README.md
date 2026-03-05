# `@goddard-ai/storage`

This package provides file resolution and token storage capabilities for the Goddard tools, particularly the `cmd` (CLI) and `daemon` (background loop) applications.

## Features

### Configuration Paths (`src/paths.ts`)

The package centralizes logic for resolving configuration paths:
- **Global Directory:** Resolves to `~/.goddard`
- **Global Config Path:** Resolves to `~/.goddard/config.ts`
- **Local Config Path:** Resolves to `goddard.config.ts` in the current working directory.
- `resolveLoopConfigPath()` automatically checks for the existence of the local config before falling back to the global config.

### Token Storage (`src/token.ts`)

It includes utilities to securely read, set, and clear tokens using JSON-based storage:
- **`FileTokenStorage`:** Default storage location for tokens, defaulting to `~/.goddard/credentials.json`.

Implements the `TokenStorage` interface from `@goddard-ai/sdk`.

## Usage

```typescript
import { getGoddardGlobalDir, resolveLoopConfigPath } from '@goddard-ai/storage';
import { FileTokenStorage } from '@goddard-ai/storage';

const globalDir = getGoddardGlobalDir();
const configPath = await resolveLoopConfigPath();

const storage = new FileTokenStorage();
await storage.setToken('my-secure-token');
const token = await storage.getToken();
```

## License

This project is licensed under the [MIT License](../LICENSE).
