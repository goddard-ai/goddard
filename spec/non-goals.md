# Non-Goals and Boundaries

## Explicit non-goals (current)

1. **Task planning intelligence**
   - `pi-loop` orchestrates cycles; it does not decide project roadmap quality.

2. **Provider-specific model management**
   - Model/provider semantics belong primarily to `pi-coding-agent`.

3. **Strong runtime isolation**
   - No built-in sandboxing, CPU/memory throttling, or process jail.

4. **Guaranteed graceful recovery**
   - Supervisory behavior (auto-restart, backoff policies) is expected from external process managers (`systemd`, etc.).

5. **Comprehensive observability stack**
   - Metrics configuration exists, but no full Prometheus exporter is currently implemented.

## Architectural boundary

`pi-loop` should remain a thin control-plane wrapper around `pi-coding-agent` and avoid duplicating agent core responsibilities.
