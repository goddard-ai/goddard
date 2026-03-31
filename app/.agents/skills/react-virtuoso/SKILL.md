---
name: react-virtuoso
description: Build, refactor, debug, and review React virtualization code that uses React Virtuoso for flat lists, grouped lists, responsive grids, virtualized tables, masonry layouts, grouped tables, window-scrolling views, or chat/message interfaces. Use when Codex needs to choose between `Virtuoso`, `GroupedVirtuoso`, `VirtuosoGrid`, `TableVirtuoso`, `GroupedTableVirtuoso`, `VirtuosoMasonry`, or `VirtuosoMessageList`, tune scrolling behavior, customize rendered wrappers, integrate UI libraries, or fix common measurement and remounting issues.
---

# react-virtuoso

Use this skill when working in codebases that render large collections with React Virtuoso or its companion packages.

## Start Here

- Inspect the current usage before editing. Determine which component is in play, how scrolling is owned, and whether the code uses `data`, `totalCount`, or a message-list `data` object.
- Load only the reference files needed for the task:
  - [components.md](./references/components.md) for component choice and base setup.
  - [scrolling-and-performance.md](./references/scrolling-and-performance.md) for load-more behavior, positioning, overscan, and jank reduction.
  - [customization-and-testing.md](./references/customization-and-testing.md) for custom wrappers, UI-library integration, and test setup.
  - [message-list.md](./references/message-list.md) only for `@virtuoso.dev/message-list`.
  - [troubleshooting.md](./references/troubleshooting.md) when debugging measurement, remounting, or ResizeObserver issues.
- Preserve the library's measuring assumptions. Avoid changes that make item height unstable unless the code explicitly compensates for it.

## Workflow

- Pick the smallest suitable component first. Avoid upgrading a flat list to a more specialized component unless the UI actually needs those behaviors.
- Preserve scroll ownership and measurement assumptions. Most Virtuoso regressions come from container sizing, unstable wrappers, or CSS that changes measured height unexpectedly.
- Prefer declarative props for scrolling behavior before reaching for imperative control.
- Keep override components and render helpers stable. Define them outside render or otherwise preserve component identity.
- Pass runtime state through `context` when custom wrappers need it.
- Supply stable keys when item identity matters, especially in object-backed lists and chat UIs.
- Filter zero-height or empty rows before handing data to Virtuoso.
- Replace protruding margins with padding inside item roots.

## Decision Rules

- Read [components.md](./references/components.md) before changing component type, introducing grouped layouts, or virtualizing a table or masonry view.
- Read [scrolling-and-performance.md](./references/scrolling-and-performance.md) before editing infinite scroll, initial positioning, overscan, viewport growth, or scroll placeholders.
- Read [customization-and-testing.md](./references/customization-and-testing.md) before changing the `components` prop, plugging in MUI or another UI kit, or making tests pass in JSDOM.
- Read [message-list.md](./references/message-list.md) before editing any chat timeline built on `@virtuoso.dev/message-list`.
- Read [troubleshooting.md](./references/troubleshooting.md) first when you see broken total height, remounting rows, zero-sized element errors, or ResizeObserver noise.
