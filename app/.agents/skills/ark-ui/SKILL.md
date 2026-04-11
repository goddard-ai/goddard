---
name: ark-ui
description: Build, refactor, debug, and review React UI code that uses Ark UI via `@ark-ui/react`, per-component `@ark-ui/react/*` packages, or `@ark-ui/react/collection`. Use when Codex must preserve Ark UI part structure and accessibility contracts, compose custom triggers or controls with `asChild`, style parts with `data-scope`, `data-part`, and state `data-*` attributes, wire `Field` or `Fieldset` context, forward refs, read component state through context hooks or `RootProvider`, or work with collection helpers such as `useAsyncList`, `createListCollection`, and `createTreeCollection`.
---

# ark-ui

Use Ark UI as headless behavior and accessibility. Let Ark own interaction logic, ARIA wiring, and structural data attributes, and add app-specific styling or state only where the surrounding code requires it.

## Start Here

- Inspect the current Ark surface first: component primitives from `@ark-ui/react`, per-component imports such as `@ark-ui/react/dialog`, or collection helpers from `@ark-ui/react/collection`.
- Preserve the existing import style and part structure before abstracting anything.
- Read [overview-and-setup.md](./references/overview-and-setup.md) before changing installation, imports, or the core Ark mental model.
- Read [composition-and-refs.md](./references/composition-and-refs.md) before using `asChild`, wrapping parts in design-system components, sharing `ids`, or forwarding refs.
- Read [styling.md](./references/styling.md) before styling Ark parts with CSS, Panda CSS, Tailwind CSS, or state data attributes.
- Read [state-management.md](./references/state-management.md) before reading inline state with `Component.Context`, calling `use*Context`, or controlling a component from outside its subtree.
- Read [forms.md](./references/forms.md) before wiring `Field`, `Fieldset`, validation, accessible labels, or React Hook Form.
- Read [collection-lists-and-async.md](./references/collection-lists-and-async.md) before managing flat option lists or remote collection data.
- Read [tree-collection.md](./references/tree-collection.md) before editing hierarchical collections, traversal, filtering, or tree mutations.

## Workflow

1. Classify the task before editing.
   - Composition or wrapper work: start with composition and refs.
   - Visual styling: start with styling.
   - In-tree state access or external control: start with state management.
   - Forms or validation: start with forms.
   - Flat or async data collections: start with list and async collections.
   - Hierarchical data: start with tree collections.
2. Keep Ark contracts intact.
   - Keep required roots, triggers, labels, positioners, and content parts present.
   - Pass a single child to `asChild`, and make that child spread props and forward refs.
   - Share `ids` when multiple composed components must point at the same DOM identity.
   - Let `Field` or `Fieldset` propagate `disabled`, `invalid`, `required`, and `readOnly` instead of manually threading those flags when possible.
   - Style via Ark's `data-scope`, `data-part`, and stateful `data-*` attributes before adding manual DOM bookkeeping.
   - Read component state with `Component.Context` or `use*Context` inside the tree. Use `useComponent` plus `RootProvider` only when control must live outside the tree.
   - Treat tree collections as immutable and replace the returned collection after mutations.
3. Implement the smallest correct change.
   - Preserve existing accessibility text, labels, helper text, and error text.
   - Preserve the repo's styling system and slot naming.
   - Keep collection item-to-value and item-to-string mapping explicit when items are not simple `{ label, value }` objects.
4. Validate the behavior that Ark is supposed to own.
   - Check keyboard and focus behavior after wrapper or trigger changes.
   - Check data-attribute driven styles after state or part changes.
   - Check ref attachment when parent code needs the rendered DOM node.
   - Check form labeling, error text, and disabled or invalid propagation.
   - Check collection traversal and value mapping after collection changes.

## Search Shortcuts

- Search `./references/composition-and-refs.md` with `rg -n "asChild|ids|ref|ark factory"`.
- Search `./references/styling.md` with `rg -n "data-scope|data-part|Panda CSS|Tailwind"`.
- Search `./references/state-management.md` with `rg -n "Context|use.*Context|RootProvider"`.
- Search `./references/forms.md` with `rg -n "Field|Fieldset|React Hook Form|ErrorText"`.
- Search `./references/collection-lists-and-async.md` with `rg -n "useAsyncList|createListCollection|itemToValue|reorder"`.
- Search `./references/tree-collection.md` with `rg -n "getNextNode|filter|insertAfter|move|flatten"`.
