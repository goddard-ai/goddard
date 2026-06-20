# IPC Errors

Read this ruleset when adding, changing, or handling daemon IPC errors, SDK/app-visible failures, client-safe error messages, diagnostic codes, or localization of daemon-backed errors.

- Treat stable client-visible error codes as contract data, not presentation copy.
- Use exported identifiers for error codes. Do not repeat error-code string literals outside the owning schema definition and tests that intentionally assert serialized values.
- Put feature-owned error-code identifiers in the owning feature schema package, such as `@goddard-ai/session/schema` or `@goddard-ai/inbox/schema`.
- Put only core daemon/backend/shared substrate error-code identifiers in `@goddard-ai/schema`.
- Do not add feature-owned error-code identifiers to `@goddard-ai/schema`.
- Client-safe structured daemon IPC errors should carry a stable code and optional safe structured details. Do not include daemon-authored fallback or presentation copy in the structured IPC envelope.
- Treat `IpcClientError.message` for structured errors as mechanical diagnostics only. Do not show it as product UI, translate it, or rely on it as fallback copy.
- App and SDK presentation should narrow by exported registries or error-code identifiers when known, map known codes exhaustively where practical, and use localized generic fallback copy for unknown codes or non-IPC failures.
- Localize user-facing app copy with Lingui in the app/client presentation layer, not in daemon service logic.
- Do not localize daemon internals, runtime logs, persisted diagnostics, agent prompts, model-facing text, or programmer errors.
- Prefer code and detail checks over parsing English error messages.
- Tests for client-visible daemon errors should assert stable codes and safe details through public IPC, SDK, or app-facing contracts. Avoid brittle assertions on English wording unless the wording itself is the presentation contract.
