# App

Read this ruleset before editing anything under `app/`.

- Treat `app/` as an Electrobun desktop app with a Bun-owned host layer and a frontend-heavy TypeScript webview.
- Read `app/glossary.md` before naming or changing app-local concepts, states, or user-facing nouns.
- Prefer app nouns that match `app/glossary.md`. Use `project` for user-added local roots unless a feature specifically requires a git repository.
- Before rewriting retained `app/plans/` documents, run the proposed change by the user for clarification.
- Reuse shared SDK, daemon, schema, and config contracts instead of inventing app-only payloads or storage models.
- Put desktop integrations behind the Electrobun RPC bridge instead of importing host APIs directly into UI code.
- UI components should render props and invoke actions, not call host APIs.
- Put app-only development tooling, fixtures, and launchable-state wiring under `src/dev/`.
- Run formatting after modifying app files.
- When a human asks for a new task, commit any app work from the previous task before starting. If that work is unfinished, include `Next step: ...` in the commit message body.

## App-Local Skills

Agents started at the repository root may not discover `app/.agents/skills/` automatically. When app work touches one of these areas, read the matching skill directly:

- `app/.agents/skills/ark-ui/SKILL.md`: Ark UI component choice, anatomy, accessibility, `asChild`, portals, presence, styling attributes, fields, collections, lists, trees, overlays, or forms.
- `app/.agents/skills/comark/SKILL.md`: Comark Markdown parsing, component syntax, AST transforms, streaming, React rendering, custom components, server/client rendering splits, or built-in plugins.
- `app/.agents/skills/electrobun/SKILL.md`: Electrobun desktop behavior, Bun main-process code, windows/views, typed RPC, Electroview, webviews, menus, tray, dialogs, sessions, shortcuts, builds, or shutdown.
- `app/.agents/skills/goddard-app-feature-planner/SKILL.md`: Drafting, updating, splitting, or reviewing app feature plans.
- `app/.agents/skills/goddard-app-sprint-planner/SKILL.md`: Sequencing or revising app implementation sprints.
- `app/.agents/skills/lexical/SKILL.md`: Lexical editors, custom nodes, commands, transforms, listeners, selections, serialization, NodeState, extensions, or React integrations.
- `app/.agents/skills/panda-css/SKILL.md`: Panda CSS styling, setup, extraction, recipes, tokens, presets, utilities, `panda.config.*`, or `styled-system`.
- `app/.agents/skills/panda-ui/SKILL.md`: Compact product UI composition in app surfaces that already use Panda CSS.
- `app/.agents/skills/preact-sigma/SKILL.md`: Code that imports `preact-sigma` or needs the relevant package docs and examples.
- `app/.agents/skills/react-virtual/SKILL.md`: TanStack React Virtual lists, window scrollers, grids, tables, sticky rows, infinite loading, measurement, or scroll positioning.
- `app/.agents/skills/tsrx/SKILL.md`: Writing, editing, or reviewing `.tsrx` files.
- `app/.agents/skills/zero-native/SKILL.md`: zero-native Zig desktop apps, manifests, frontend sources, bridge commands, security policy, windows, dialogs, automation, or distribution.

## Repository App Skills

These app-oriented skills live under the repository-level `.agents/skills/` directory:

- `.agents/skills/app-forms/SKILL.md`: app forms, dialog forms, form models, async fields, pending UI, and submit payloads.
- `.agents/skills/app-implementation-patterns/SKILL.md`: app state ownership, contextual mutations, hooks, async work, cross-domain coordination, surface composition, TSRX organization, and alignment.
- `.agents/skills/app-tsrx-pages/SKILL.md`: page-like TSRX components, page skeletons, TSRX control flow, query data contexts, page models, task helpers, and loading/error UI.
