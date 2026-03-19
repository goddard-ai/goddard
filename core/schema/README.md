# @goddard-ai/schema

This package contains shared communication types and their Zod validation schemas for the Goddard AI project. It is used to enforce strict data validation between the backend and SDK.

## Usage

```typescript
import { CreatePrInput } from "@goddard-ai/schema/backend"

// Validate payload
const input = CreatePrInput.parse(payload)
```

## License

This project is licensed under the [MIT License](./LICENSE-MIT).
