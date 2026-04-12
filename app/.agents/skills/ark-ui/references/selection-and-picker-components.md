# Selection and Picker Components

Use this reference for collection-backed or picker-like React Ark components:

- `Select`
- `Combobox`
- `Listbox`
- `Tags Input`
- `Tree View`
- `Date Picker`
- `Color Picker`
- `File Upload`
- `Signature Pad`
- `Image Cropper`

## Collection-Backed Widgets

`Select`, `Combobox`, `Listbox`, and many `Tags Input` flows work best with explicit collections.

- Prefer `createListCollection` when you already have final items.
- Keep `itemToValue`, `itemToString`, and disabled-item logic explicit when items are custom objects.
- Preserve hidden form elements such as `HiddenSelect` when the component exposes them.
- Keep `Positioner` and `Content` anatomy intact for popup variants such as `Select` and `Combobox`.

Choose by interaction:

- `Select`: choose from a collection without free typing.
- `Combobox`: type to filter or search a collection.
- `Listbox`: persistent visible selection surface.
- `Tags Input`: multiple token values with text entry and item selection.

## `Tree View`

Use `Tree View` with `createTreeCollection` when the UI itself is hierarchical.

- Preserve node, branch, item, and branch-content anatomy.
- Keep collection identity stable so expansion and selection do not reset accidentally.
- Prefer `Tree View` over generic nested lists when keyboard navigation and tree semantics matter.

## `Date Picker`

Use `Date Picker` for single, multiple, or range calendar selection.

- `selectionMode` drives single, multiple, or range behavior.
- `locale` and `startOfWeek` affect labels and calendar layout.
- Preserve view controls, triggers, and table parts when customizing.

## `Color Picker`

Use `Color Picker` when the app needs parsed color values, channel controls, swatches, or format-aware editing.

- Treat parsed color state as structured data, not an arbitrary string.
- Preserve channel-input and swatch anatomy when styling.

## `File Upload`

Use `File Upload` for drag-and-drop zones, file picker triggers, and upload state display.

- Keep file-selection logic and form-submission semantics aligned.
- Preserve hidden inputs or triggers that actually connect to the file dialog.

## `Signature Pad`

Use `Signature Pad` when pointer input itself is the value.

- Preserve canvas and control anatomy.
- Treat clearing, exporting, or submitting signature data as app-level concerns.

## `Image Cropper`

Use `Image Cropper` when the user needs an editable crop region, not just a static preview.

- Preserve `Viewport`, `Image`, `Selection`, `Handle`, and grid anatomy.
- Keep crop state and exported crop data separate from the original asset.

## Cross-Cutting Rules

- Prefer `Highlight` and locale-aware filtering helpers for searchable item text instead of manual substring slicing.
- Use `Portal` for popup-bearing pickers unless the current code intentionally keeps content in-tree.
- Validate keyboard navigation, selected values, and hidden form elements after refactors.
