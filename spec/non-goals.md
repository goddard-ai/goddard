---
id: non-goals-boundaries
status: ACTIVE
links:
  - type: Relates-To
    target: spec/vision.md
  - type: Relates-To
    target: spec/product.md
---

# Non-Goals and Boundaries

## Explicit Non-Goals (Current Scope)

### 1. Task planning intelligence
Goddard orchestrates cycles and surfaces GitHub events; it does not decide project roadmap quality, prioritize issues, or evaluate code correctness. Those decisions belong to the agent and the human operator.

### 2. Provider-specific model management
Model and provider semantics (API keys, context windows, pricing tiers) belong to `pi-coding-agent`. Goddard's loop layer passes a model string through without interpreting it.

### 3. Strong runtime isolation
No built-in sandboxing, CPU/memory throttling, or process-level jailing. Resource constraints are enforced at the application layer (token limits, rate limits) and at the OS layer (`systemd` `Nice`, external cgroups, etc.).

### 4. Guaranteed graceful recovery
Automatic restart, backoff-after-crash, and health-check supervision are the responsibility of external process managers (`systemd`, Kubernetes, etc.). The loop runtime exposes clean exit codes and deterministic error messages to support external supervisors; it does not implement supervisor logic internally.

### 5. Comprehensive observability stack
`metrics.prometheusPort` is a configuration surface, but no full Prometheus exporter is currently implemented. Basic logging (`metrics.enableLogging`) is available. A complete metrics pipeline is tracked in [`product.md`](./product.md) under Planned Evolution.

### 6. GitHub platform completeness
Goddard does not aim to replicate the full GitHub API surface. It provides targeted, opinionated integrations (PR creation, reaction automation, event streaming) rather than a general-purpose API client.

### 7. Social / OAuth login
Authentication is GitHub Device Flow only. Social login providers (Google, GitLab, etc.) are explicitly out of scope.

---

## Architectural Boundary

Goddard — both the platform backend and the autonomous loop layer — must remain a **thin control plane** around `pi-coding-agent` and the GitHub API.

It must not:
- Duplicate agent core responsibilities (prompt generation, model interaction, code editing).
- Attempt to re-implement GitHub's platform logic.
- Absorb features that properly belong to `pi-coding-agent`.

Features that belong in `pi-coding-agent` should be contributed there, not absorbed here. This boundary is enforced by design in the SDK-first architecture — see [`adr/001-sdk-first-architecture.md`](./adr/001-sdk-first-architecture.md).
