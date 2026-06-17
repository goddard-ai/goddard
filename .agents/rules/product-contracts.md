# Product Contracts

Read this ruleset when touching product behavior, `spec/`, SDK/app parity, shared data loading or mutation, system configuration, or user-visible capabilities.

- `spec/` is the canonical source of product behavior and intent.
- Do not edit `spec/` unless explicitly asked.
- Do not knowingly let code, docs, or tests diverge from `spec/`.
- If a request conflicts with `spec/`, call it out instead of silently working around it.
- New user-facing capability in `app/` that depends on shared data loading, shared data mutation, or system configuration must also be implemented in `core/sdk/` in the same PR.
- UI-only behavior does not require `core/sdk/` parity.
- Do not ship `app/` ahead of `core/sdk/` when shared data or system configuration behavior is involved.
