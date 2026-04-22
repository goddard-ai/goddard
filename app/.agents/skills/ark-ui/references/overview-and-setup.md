# Overview and Setup

## Mental Model

- Treat Ark UI as a headless component library: keep accessibility and interaction behavior in Ark, and keep presentation in app code.
- Treat Ark UI components as part-based building blocks backed by Zag.js state machines.
- Preserve required part structure when wrapping or abstracting components into a design system.

## Upstream Docs to Remember

- `Getting Started`: install Ark UI and sanity-check the first component setup.
- `About`: confirm the cross-framework and Zag.js mental model when architecture questions arise.
- `Changelog`: check version-specific behavior before assuming a component or prop has always worked the same way.
- `MCP Server`: useful only when the surrounding environment is wiring Ark's official MCP server for tooling or automation.
- `LLMs.txt`: the upstream React docs export that mirrors the official component, collection, and utility surface.

## Install and Import

Install Ark UI when it is not already available:

```bash
npm install @ark-ui/react
```

Choose the import style that matches the repo and keep it consistent inside the touched area:

```tsx
import { Dialog } from "@ark-ui/react";
import { Slider } from "@ark-ui/react/slider";
import { createListCollection } from "@ark-ui/react/collection";
```

## First Component Checklist

- Add the root and the required part components for the primitive you are using.
- Keep visible labels, descriptions, and triggers explicit.
- Style parts with Ark's data attributes instead of assuming generated class names.
- Verify keyboard behavior, focus behavior, and ARIA relationships after composition changes.

## Ownership Boundary

Let Ark own:

- structural data attributes such as `data-scope`, `data-part`, and stateful `data-*` flags
- the accessibility wiring between parts
- component-local interaction behavior

Keep app code responsible for:

- visual styling and design tokens
- persistence and remote data fetching
- coordination between multiple unrelated components
