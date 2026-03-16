# Daemon Multi-session Execution Map

## Context / Why

### Problem statement
This work has a large blast radius across storage, runtime orchestration, daemon routing, and lifecycle security. A single monolithic implementation is high-risk and hard to review.

### Why this split
The split isolates concerns and enables safe parallelization after one shared prerequisite (contract freeze).

### Success condition
Parallel tracks can ship independently with minimal merge churn, then integrate in one controlled cutover phase.

## Order of execution
1. **Plan 0** (serial prerequisite)
2. **Plan A / Plan B / Plan C** (parallel)
3. **Plan D** (serial integration + cutover)

## Parallelization boundaries

### Plan A owns
- DB/storage identity semantics (`id` + `acpId`)
- storage lookup API migration

### Plan B owns
- multi-session runtime engine in `core/session`
- per-session ACP lifecycle behavior

### Plan C owns
- daemon HTTP/WS control-plane routes
- session route wiring contracts

### Plan D owns
- final wiring across A/B/C
- lifecycle security cleanup guarantees
- dead-path removal and final cutover

## Merge strategy
- Merge Plan 0 first.
- A/B/C can merge in any order after Plan 0 (resolve minor conflicts as needed).
- Plan D merges last and is the stabilization gate.

## Non-goals for this split
- `cmd/` work
- desktop `app/` UX work
