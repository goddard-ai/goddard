---
name: ark-ui
description: Build, refactor, debug, and review React UI code that uses Ark UI via `@ark-ui/react` or `@ark-ui/react/*`. Use when Codex must choose the right React Ark component, collection helper, or utility; preserve part anatomy and accessibility contracts; compose with `asChild`, `Portal`, `Presence`, or `EnvironmentProvider`; style parts with `data-scope`, `data-part`, CSS variables, and state `data-*` attributes; wire `Field` or `Fieldset`; work with `createListCollection`, `useAsyncList`, `useListSelection`, or `createTreeCollection`; or handle React Ark surfaces such as dialogs, menus, selects, comboboxes, date pickers, file upload, image cropper, tree view, splitter, toast, tour, locale, download, focus trap, formatting, and related utilities.
---

# ark-ui

Use this skill for React Ark UI work only. Let Ark own interaction logic, ARIA wiring, focus rules, and structural data attributes, and add app-specific styling or state only where the surrounding code requires it.

## Start Here

- Inspect the current Ark surface first: component primitives from `@ark-ui/react`, per-component imports such as `@ark-ui/react/dialog`, or collection helpers from `@ark-ui/react/collection`.
- Preserve the existing import style and part structure before abstracting anything.
- Read [choose-components-and-utilities.md](./references/choose-components-and-utilities.md) first when the real question is which Ark React surface to use.
- Read [overview-and-setup.md](./references/overview-and-setup.md) before changing installation, imports, or the core Ark mental model.
- Read [composition-and-refs.md](./references/composition-and-refs.md) before using `asChild`, wrapping parts in design-system components, sharing `ids`, or forwarding refs.
- Read [styling.md](./references/styling.md) before styling Ark parts with CSS, Panda CSS, Tailwind CSS, or state data attributes.
- Read [state-management.md](./references/state-management.md) before reading inline state with `Component.Context`, calling `use*Context`, or controlling a component from outside its subtree.
- Read [animation-and-presence.md](./references/animation-and-presence.md) before coordinating entry and exit animation, `present`, `lazyMount`, `unmountOnExit`, or `Presence`.
- Read [forms.md](./references/forms.md) before wiring `Field`, `Fieldset`, validation, accessible labels, or React Hook Form.
- Read [collection-lists-and-async.md](./references/collection-lists-and-async.md) before managing flat option lists or remote collection data.
- Read [list-selection.md](./references/list-selection.md) before managing independent list selection state, multi-select behavior, or range-like selection flows outside a built-in Ark component.
- Read [tree-collection.md](./references/tree-collection.md) before editing hierarchical collections, traversal, filtering, or tree mutations.
- Read [overlays-and-transient-ui.md](./references/overlays-and-transient-ui.md) before editing dialogs, popovers, menus, hover cards, tooltips, tours, floating panels, or toast flows.
- Read [selection-and-picker-components.md](./references/selection-and-picker-components.md) before editing selects, comboboxes, listboxes, tags input, date pickers, color pickers, file upload, signature pad, image cropper, or tree view.
- Read [form-and-value-components.md](./references/form-and-value-components.md) before editing checkbox-like controls, toggles, rating, sliders, numeric inputs, pin inputs, password inputs, editable text, or timer components.
- Read [structure-navigation-and-feedback.md](./references/structure-navigation-and-feedback.md) before editing disclosure, navigation, viewport, progress, clipboard, avatar, QR, or marquee behavior.
- Read [utilities-and-runtime.md](./references/utilities-and-runtime.md) before editing client-only rendering, download, environment, focus trap, formatting, frame, highlight, locale, JSON tree view, or swap behavior.

## Workflow

1. Classify the task before editing.
   - Component or utility choice: start with the chooser reference.
   - Composition or wrapper work: start with composition and refs.
   - Visual styling: start with styling.
   - In-tree state access or external control: start with state management.
   - Entry and exit animation or mount timing: start with animation and presence.
   - Forms or validation: start with forms.
   - Flat collections or async loading: start with list and async collections.
   - Independent selection state: start with list selection.
   - Hierarchical data: start with tree collections.
   - Overlays or transient UI: start with overlays.
   - Pickers or selection widgets: start with picker components.
   - Value controls: start with form and value components.
   - Structure, navigation, or feedback primitives: start with structure and feedback.
   - Runtime or display helpers: start with utilities and runtime.
2. Keep Ark contracts intact.
   - Keep required roots, triggers, labels, positioners, and content parts present.
   - Pass a single child to `asChild`, and make that child spread props and forward refs.
   - Share `ids` when multiple composed components must point at the same DOM identity.
   - Let `Field` or `Fieldset` propagate `disabled`, `invalid`, `required`, and `readOnly` instead of manually threading those flags when possible.
   - Style via Ark's `data-scope`, `data-part`, and stateful `data-*` attributes before adding manual DOM bookkeeping.
   - Read component state with `Component.Context` or `use*Context` inside the tree. Use `useComponent` plus `RootProvider` only when control must live outside the tree.
   - Preserve `Portal`, `Positioner`, `Backdrop`, hidden form elements, and collection props where the component family expects them.
   - Use Ark's collection helpers instead of rebuilding item lookup, selection state, or tree traversal by hand.
   - Use `EnvironmentProvider`, `FocusTrap`, `Presence`, and format helpers when the repo already leans on Ark runtime utilities instead of introducing parallel abstractions.
   - Treat tree collections as immutable and replace the returned collection after mutations.
3. Implement the smallest correct change.
   - Preserve existing accessibility text, labels, helper text, and error text.
   - Preserve the repo's styling system and slot naming.
   - Keep collection item-to-value and item-to-string mapping explicit when items are not simple `{ label, value }` objects.
   - Prefer React examples, React hooks, and React package entrypoints only.
4. Validate the behavior that Ark is supposed to own.
   - Check keyboard and focus behavior after wrapper or trigger changes.
   - Check data-attribute driven styles after state or part changes.
   - Check ref attachment when parent code needs the rendered DOM node.
   - Check form labeling, error text, and disabled or invalid propagation.
   - Check collection traversal and value mapping after collection changes.
   - Check portal, presence, and mount timing after overlay or animation changes.
   - Check locale, download, formatting, or custom environment behavior when touching utilities.

## Search Shortcuts

- Search `./references/choose-components-and-utilities.md` with `rg -n "Dialog|Select|Tree View|Swap|Focus Trap|JSON Tree View"`.
- Search `./references/composition-and-refs.md` with `rg -n "asChild|ids|ref|ark factory"`.
- Search `./references/styling.md` with `rg -n "data-scope|data-part|Panda CSS|Tailwind"`.
- Search `./references/state-management.md` with `rg -n "Context|use.*Context|RootProvider"`.
- Search `./references/animation-and-presence.md` with `rg -n "present|lazyMount|unmountOnExit|data-state"`.
- Search `./references/forms.md` with `rg -n "Field|Fieldset|React Hook Form|ErrorText"`.
- Search `./references/collection-lists-and-async.md` with `rg -n "useAsyncList|createListCollection|itemToValue|reorder"`.
- Search `./references/list-selection.md` with `rg -n "useListSelection|selectionMode|deselectable|toggle"`.
- Search `./references/tree-collection.md` with `rg -n "getNextNode|filter|insertAfter|move|flatten"`.
- Search `./references/overlays-and-transient-ui.md` with `rg -n "Dialog|Popover|Menu|Toast|Tour|Portal"`.
- Search `./references/selection-and-picker-components.md` with `rg -n "Select|Combobox|Date Picker|File Upload|Tree View|Image Cropper"`.
- Search `./references/form-and-value-components.md` with `rg -n "Checkbox|Slider|Number Input|Pin Input|Editable|Timer"`.
- Search `./references/structure-navigation-and-feedback.md` with `rg -n "Accordion|Tabs|Scroll Area|Splitter|Progress|Clipboard|QR Code"`.
- Search `./references/utilities-and-runtime.md` with `rg -n "Client Only|Download Trigger|Environment|Focus Trap|Format|Locale|Swap"`.
