# IPC Errors

Read this ruleset when adding, changing, or handling daemon IPC errors, SDK/app-visible failures, client-safe error messages, diagnostic codes, or localization of daemon-backed errors.

- Treat stable client-visible error codes as contract data, not presentation copy.
- Use exported identifiers for error codes. Do not repeat error-code string literals outside the owning schema definition and tests that intentionally assert serialized values.
- Put feature-owned error-code identifiers in the owning feature schema package, such as `@goddard-ai/session/schema` or `@goddard-ai/inbox/schema`.
- Put only core daemon/backend/shared substrate error-code identifiers in `@goddard-ai/schema`.
- Do not add feature-owned error-code identifiers to `@goddard-ai/schema`.
- Client-safe daemon IPC errors should carry a stable code, optional safe structured details, and a fallback/debug message.
- App and SDK presentation should branch on exported error-code identifiers when known and treat daemon messages as fallback or debugging text.
- Localize user-facing app copy with Lingui in the app/client presentation layer, not in daemon service logic.
- Do not localize daemon internals, runtime logs, persisted diagnostics, agent prompts, model-facing text, or programmer errors.
- Prefer code and detail checks over parsing English error messages.
- Tests for client-visible daemon errors should assert stable codes and safe details through public IPC, SDK, or app-facing contracts. Avoid brittle assertions on fallback English wording unless the wording itself is the contract.
