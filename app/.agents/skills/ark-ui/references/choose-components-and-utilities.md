# Choose Components and Utilities

Use this reference when the real question is which Ark React surface should own the interaction.

## Core Guides

- `Styling`: part selectors, state selectors, CSS variables, and styling-system integration.
- `Composition`: `asChild`, shared `ids`, and wrapper-component contracts.
- `Component State`: `Component.Context`, `use*Context`, `useComponent`, and `RootProvider`.
- `Animation`: CSS or JS animation coordinated with `present`, `lazyMount`, and `unmountOnExit`.
- `Forms`: `Field`, `Fieldset`, labels, errors, and form-library integration.
- `Refs`: reading the rendered DOM node from React.

## Collections

- `List Collection`: flat item arrays with lookup, traversal, and disabled-item metadata.
- `Async List`: remote or filtered data with loading, error, pagination, and cancellation support.
- `List Selection`: selection state for custom lists outside a built-in Ark widget.
- `Tree Collection`: immutable hierarchical data for file trees, navigation trees, and tree views.

## Component Families

### Disclosure, Navigation, and Layout

- `Accordion`: stacked disclosure with single or multiple expanded items.
- `Collapsible`: one independently collapsible region.
- `Tabs`: in-place panel switching with trigger, list, and content parts.
- `Steps`: wizard-like progress through ordered stages.
- `Pagination`: page navigation controls and page-range helpers.
- `Carousel`: slide-based navigation and viewport control.
- `Scroll Area`: custom scrollbars and scroll viewport primitives.
- `Splitter`: resizable panel layouts.
- `Marquee`: continuously scrolling content.

### Overlays and Transient UI

- `Dialog`: modal or non-modal dialog content.
- `Popover`: anchored content that behaves like a lightweight dialog.
- `Tooltip`: non-interactive descriptive hover and focus hint.
- `Hover Card`: richer hover-triggered preview content.
- `Menu`: action menus, nested menus, and context-style command lists.
- `Floating Panel`: draggable or floating tool panels with persistent chrome.
- `Toast`: transient status messaging.
- `Tour`: guided multi-step onboarding overlays.

### Selection, Pickers, and Composite Inputs

- `Select`: button-style single or multiple choice backed by a collection.
- `Combobox`: text input plus filtered collection and optional free typing.
- `Listbox`: visible collection selection without trigger-plus-popup chrome.
- `Tags Input`: tokenized multi-value text entry.
- `Tree View`: navigable hierarchical collection UI backed by `createTreeCollection`.
- `Date Picker`: calendar-driven single, multiple, or range date selection.
- `Color Picker`: parsed color values, channels, swatches, and format switching.
- `File Upload`: drag-and-drop or picker-based file selection.
- `Signature Pad`: pointer-based signature capture.
- `Image Cropper`: image selection, crop handles, and crop region editing.

### Form and Value Controls

- `Checkbox`: boolean or grouped multi-select check state.
- `Radio Group`: one-of-many choice.
- `Switch`: binary setting with switch semantics.
- `Segment Group`: mutually exclusive segmented options.
- `Toggle`: independent on and off button state.
- `Toggle Group`: exclusive or multi-toggle clusters.
- `Rating Group`: star or score-like rating selection.
- `Slider`: one-dimensional range value.
- `Angle Slider`: circular range value.
- `Number Input`: typed and stepped numeric value.
- `Pin Input`: segmented OTP or verification code entry.
- `Password Input`: password visibility and password-field affordances.
- `Editable`: inline view and edit switching.
- `Timer`: stopwatch or countdown-like time state and controls.

### Display and Feedback

- `Avatar`: image plus fallback identity display.
- `Clipboard`: copy-to-clipboard trigger and copied-state feedback.
- `Progress - Circular`: circular progress indicator.
- `Progress - Linear`: bar progress indicator.
- `QR Code`: generated QR rendering and download trigger.

## Utilities

- `Client Only`: client-only rendering with optional fallback.
- `Download Trigger`: programmatic file downloads from text, blobs, or URLs.
- `Environment`: DOM-environment bridging for iframe, Shadow DOM, or Electron.
- `Focus Trap`: trap keyboard focus inside a region.
- `Format Byte`: byte-size formatting.
- `Format Time`: time formatting from strings or `Date`.
- `Format Relative Time`: relative time formatting via `Intl.RelativeTimeFormat`.
- `Frame`: render content inside an iframe with lifecycle hooks.
- `Highlight`: emphasize matching substrings in rendered text.
- `JSON Tree View`: inspect JSON data as a tree UI.
- `Locale`: locale-aware formatting and filtering helpers.
- `Presence`: mount and visibility control for animation flows.
- `Swap`: swap between on and off indicators with animation-friendly structure.

## Decision Rules

- Need a popup anchored to another element: start with `Popover`, `Tooltip`, `Hover Card`, `Menu`, or `Select`, not a custom absolute-positioned div.
- Need blocking content or full focus management: start with `Dialog`; use non-modal dialog only when outside interaction must remain available.
- Need text filtering plus options: start with `Combobox`; use `Select` when typing is not part of the interaction.
- Need a permanently visible selectable list: start with `Listbox`.
- Need raw selection state for your own markup: use `useListSelection` plus a collection instead of rebuilding toggling logic.
- Need hierarchical UI: use `createTreeCollection` plus `Tree View`; use `JSON Tree View` only for generic data inspection.
- Need SSR or custom DOM environment handling: use `Client Only`, `EnvironmentProvider`, `FocusTrap`, `Presence`, or `Portal` instead of hand-rolled effects.
