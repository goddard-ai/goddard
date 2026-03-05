# @goddard-ai/schema

This package contains shared communication types and their Zod validation schemas for the Goddard AI project. It is used to enforce strict data validation between the backend and SDK.

## Usage

```typescript
import { CreatePrInputSchema, type CreatePrInput } from "@goddard-ai/schema";

// Validate payload
const input = CreatePrInputSchema.parse(payload);
```

## License

This project is licensed under the [MIT License](../LICENSE).
