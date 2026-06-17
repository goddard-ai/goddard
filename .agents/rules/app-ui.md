# App UI

Read this ruleset when editing app UI, Panda/Ark composition, styling, tab surfaces, dialogs, component naming, JSX/TSRX formatting constraints, or visual treatment.

- When adding UI components or interactive elements in `app/`, use the local `panda-ui` skill to align Panda CSS and Ark UI composition with the existing design system.
- Do not give tab-level views or full-tab page wrappers their own background by default. The app shell owns the themed canvas.
- Apply visual treatment to bounded local elements such as cards, headers, panels, or bubbles instead of repainting the whole tab.
- Keep appearance controls inline on the settings detail tab. Do not move them into a separate dialog or modal.
- Avoid `<Suspense>` elements in TSRX components. Prefer TSRX try-pending blocks for pending UI.
- TSRX allows Preact hooks anywhere in the component render scope, including after early returns, inside `if`/`for` blocks, and inside JSX children. Keep hook calls near their natural use site instead of hoisting them solely for React-style hook ordering.
- When a dialog component accepts `dialog: UseDialogReturn`, treat it as content rendered under a parent `Dialog.RootProvider`; do not nest `Dialog.Root` inside that component.
- Do not blur modal or dialog backdrops. Prefer a mostly opaque flat color derived from the current theme background.
- Prefer the `class` JSX prop over `className`.
- Prefer the global `preact.` namespace for Preact types such as `preact.ComponentChildren` instead of importing those types directly.
- Prefer named exports for TSRX components. Use `export default` only when a framework or tooling contract requires a default export.
- Prefix reusable, pre-styled UI primitives with `Good` such as `GoodTooltip`. Reserve that prefix for opinionated design-system components, not feature/domain modules or state.
- In Panda style objects, prefer tokenized border shorthands such as `border: "1px solid {colors.border}"` over split declarations like `border: "1px solid"` plus `borderColor: "border"` when the border width, style, and token color are fixed together.
- Never destructure component props. Define component prop types inline instead of creating `Props` aliases or interfaces.
- Avoid assigning JSX/TSRX elements to local `const` variables. Prefer inlining elements at the use site, including inline `<tsrx>` expression blocks when needed.
- Keep a blank line between sibling JSX tags and non-JSX statements for readability, including sibling `if` blocks after JSX. Consecutive JSX tags do not need a blank line between them, and parent opening tags do not need a blank line before their first child statement.
- Keep single-use event handlers at the use site. Prefer inline JSX callbacks for tiny handlers; for larger handlers, declare a block-scoped arrow function in the nearest JSX or control-flow block that uses it.
- Prefer passing async event handlers directly, such as `onClick={saveChanges}`, instead of wrapping them only to discard the returned promise with `void`.
- Avoid local aliases that only hide useful ownership or reactivity context such as `page.*` or `*.value`; introduce locals when they name domain meaning, avoid duplicated non-trivial logic, or are reused enough to improve clarity.
- Do not add module-level first-line description comments or routine component description comments unless a comment explains non-obvious behavior that the code itself does not make clear.
- Use all-lowercase kebab-case folder names for UI feature trees.
- Use all-lowercase kebab-case component filenames and avoid repeating the parent feature name in child component names.
- Name style modules after the component or surface they style, using `.style.ts` filenames such as `message-list.style.ts`; avoid generic `styles.ts` filenames.
- Do not use bare generic component names like `List`, `View`, `Page`, or `Dialog`; include feature-specific context in exported component names.
- Keep feature components and their sigma state modules together inside feature folders. Do not add barrel modules there, and do not create `state/` subfolders.
