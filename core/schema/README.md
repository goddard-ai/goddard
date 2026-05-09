# @goddard-ai/schema

This package contains schemas for Goddard's core daemon, backend, and shared substrate contracts. Feature-owned product schemas live in the owning feature package's `./schema` entrypoint instead of being added or re-exported here.

## Related Docs

- [Schema Glossary](./glossary.md)

## Usage

```typescript
import { CreatePrInput } from "@goddard-ai/schema/backend"

// Validate payload
const input = CreatePrInput.parse(payload)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
