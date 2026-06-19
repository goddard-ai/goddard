# Loops

> A loop is a reusable daemon-owned runtime that can continue beyond a single prompt or action run. This page explains how clients start, inspect, list, and shut down loop runtimes without owning their state directly.

## Core idea

- A loop is a named reusable automation runtime owned by the daemon.
- Clients can start, inspect, list, and shut down loop runtimes.

## Start and reuse

- Starting a loop creates or reuses the daemon-owned runtime for the requested context.
- Clients should not create parallel watcher state for the same loop.
- Reuse means clients should inspect the daemon's loop state before assuming a fresh runtime exists.
- Shutting down a loop stops that daemon-owned runtime without redefining the loop's persisted defaults.

## Configuration

- Loops resolve persisted configuration before runtime behavior begins.
- A loop may be represented by prompt content or by a richer packaged definition.
- Persisted loop defaults live in machine-readable configuration associated with the loop.
- Runtime input can influence one start request without becoming the loop's saved default.

## Boundaries

- Loops are for reusable runtime behavior that may continue until shut down.
- Invalid persisted configuration should not replace the daemon's last valid behavior for future loop resolutions.
- Loop state is daemon-owned; clients recover from missed updates by listing or inspecting loops again.
