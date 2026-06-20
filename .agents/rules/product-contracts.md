# Product Contracts

Read this ruleset when touching product behavior, `spec/`, SDK/app parity, shared data loading or mutation, system configuration, or user-visible capabilities.

- `spec/` is the canonical source of product intent.
- Public `docs/` are documentation for current supported behavior and may intentionally diverge from `spec/` when the spec describes an ideal or future shape.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, non-public docs, or tests diverge from `spec/`.
- If a request conflicts with `spec/`, call it out instead of silently working around it.
- New user-facing capability in `app/` that depends on shared data loading, shared data mutation, or system configuration must also be implemented in `core/sdk/` in the same PR.
- UI-only behavior does not require `core/sdk/` parity.
- Do not ship `app/` ahead of `core/sdk/` when shared data or system configuration behavior is involved.
- App-visible daemon errors are product contracts. Use stable exported error-code identifiers and do not make app behavior depend on parsing English daemon messages.
