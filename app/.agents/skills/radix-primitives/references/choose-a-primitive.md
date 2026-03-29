# Choose A Primitive

Use this file when the task is really about pattern selection, not syntax.

## Quick Picks

- Use `Dialog` for modal or layered task flows that take over user attention.
- Use `AlertDialog` for destructive confirmation or other decisions that need a clear cancel path.
- Use `Popover` for small interactive surfaces anchored to a trigger, such as filter controls or inspectors.
- Use `Tooltip` for short explanatory text only. Do not turn it into a mini-popover.
- Use `HoverCard` for supplemental preview information shown on hover or focus.
- Use `DropdownMenu` for command menus from a button or trigger.
- Use `ContextMenu` for right-click or context-triggered commands.
- Use `Menubar` for persistent application or document menus.
- Use `Select` for button-triggered single selection with typeahead and menu-style behavior.
- Use `RadioGroup` for a small always-visible set of mutually exclusive options.
- Use `Checkbox` for independent boolean or multi-select choices.
- Use `Switch` for an immediate on/off setting.
- Use `Toggle` for a single pressed-state control, usually icon-based.
- Use `ToggleGroup` for a coordinated set of pressed-state controls.
- Use `Slider` for numeric range selection by dragging.
- Use `Form` when you want native constraint validation plus accessible inline messages.
- Use `Accordion` for repeated disclosure items in a vertical stack.
- Use `Collapsible` for one show or hide region.
- Use `Tabs` for switching between peer panels in place.
- Use `NavigationMenu` for higher-level site or app navigation with rich dropdown content.
- Use `Toast` for transient, non-blocking feedback that is safe to ignore.
- Use `Progress` for task completion status.
- Use `ScrollArea` when native scrolling is fine but the visual scroll affordance needs custom chrome.
- Use `AspectRatio`, `Avatar`, and `Separator` for layout or presentation helpers.
- Use `Portal`, `Slot`, `VisuallyHidden`, `AccessibleIcon`, and `Direction.Provider` as utilities that support other primitives or your own components.

## Common Boundaries

- Prefer `Tooltip` over `Popover` only when the content is brief, non-interactive, and tied to hover or focus.
- Prefer `Popover` over `Dialog` when the interaction is small and anchored, and the rest of the page should remain visually present.
- Prefer `AlertDialog` over `Dialog` when the primary job is confirming a dangerous or irreversible action.
- Prefer `Select` over `DropdownMenu` when the user is choosing a value for form state, not issuing commands.
- Prefer `RadioGroup` over `Select` when the option set is small enough to keep visible.
- Prefer `Tabs` over `Accordion` when exactly one panel should be visible in a shared region.
- Prefer `Collapsible` over `Accordion` when there is only one disclosure region and no item list semantics.
- Prefer `Toast` over dialog-based feedback only when the user does not have to answer before continuing.

## Family Map

### Overlays And Menus

- `Dialog`: general modal content, forms, editors, and layered workflows.
- `AlertDialog`: confirmation and destructive actions, with explicit cancel and action parts.
- `Popover`: anchored content, often interactive, with optional arrow and collision handling.
- `HoverCard`: lightweight preview card.
- `Tooltip`: terse labels or hints.
- `DropdownMenu`, `ContextMenu`, `Menubar`: command surfaces with labels, separators, submenus, and item groups.

### Inputs And Selection

- `Form`: native validation API plus accessible messages and managed focus.
- `Label`: accessible labelling for native-based controls.
- `Checkbox`, `RadioGroup`, `Switch`: boolean or single-choice controls.
- `Slider`: numeric value or range control with one or more thumbs.
- `Select`: menu-driven single selection with typeahead and grouping.
- `Toggle`, `ToggleGroup`: pressed-state controls, often toolbar-style.
- `One-Time Password Field`, `Password Toggle Field`: specialized auth-entry primitives when the codebase already uses them.

### Navigation, Disclosure, And Feedback

- `Accordion`, `Collapsible`: show and hide content.
- `Tabs`: peer panel switching.
- `NavigationMenu`: top-level nav with viewport and indicator patterns.
- `Toolbar`: grouped action controls.
- `Toast`: transient announcements and safe secondary actions.
- `Progress`: visual progress state.

### Layout And Utility

- `AspectRatio`: keep media or boxes at a fixed ratio.
- `Avatar`: image with fallback.
- `ScrollArea`: custom scroll chrome around native scrolling.
- `Separator`: visual or semantic divider.
- `Portal`: render outside normal DOM ancestry.
- `Slot`: support your own `asChild` API.
- `VisuallyHidden`: keep content accessible but off-screen.
- `AccessibleIcon`: add meaning to otherwise decorative icon-only controls.
- `Direction.Provider`: make primitives inherit RTL or LTR behavior globally.
