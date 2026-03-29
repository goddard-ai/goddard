# Forms, Selection, And Inputs

Use this file when the task changes validation, labels, checked state, selected values, or range inputs.

## `Form`

- Use `Form.Root` plus nested `Field` blocks when you want native constraint validation with accessible inline messages.
- Keep `Field`, `Label`, `Control`, `Message`, and `Submit` aligned by a shared `name`.
- Use `match` on `Form.Message` for native validity states such as `valueMissing` and `typeMismatch`.
- Pass a function to `match` for custom validation. It receives the current field value first and the full `FormData` second, and it may be async.
- Use `ValidityState` when styling or rendering depends on the field's raw validity object.
- Use `data-valid` and `data-invalid` for styling before duplicating validity state in React.
- For server-side validation, use `forceMatch`, `serverInvalid`, and `onClearServerErrors` instead of custom one-off plumbing.
- Do not assume `Form` composes cleanly with other Radix form primitives such as `Checkbox` or `Select`. The docs call out that limitation explicitly.

## `Label`

- Use `Label.Root` with `htmlFor` or by wrapping the control.
- Keep custom controls based on native elements such as `button` or `input`, otherwise native labelling behavior may break.
- Prefer `Label` over handwritten spans when the control needs a visible and accessible label.

## Boolean And Choice Controls

### `Checkbox`

- Preserve `Checkbox.Root` and `Checkbox.Indicator`.
- Pair it with a visible label through `Label` or `htmlFor`.
- Use for independent choices or multi-select lists, not mutually exclusive values.

### `RadioGroup`

- Preserve `RadioGroup.Root`, `Item`, and `Indicator`.
- Give each item its own `value` and label the whole group or provide `aria-label`.
- Use for a small set of mutually exclusive options kept on screen.

### `Switch`

- Preserve `Switch.Root` and `Thumb`.
- Use `checked` or `defaultChecked` plus `onCheckedChange` the same way you would for checkbox-like state.
- Expect an `input` to render automatically inside forms so events propagate like a native control.
- Style through `data-state="checked|unchecked"` and `data-disabled`.

### `Toggle` And `ToggleGroup`

- Use `Toggle` for a single pressed-state action.
- Use `ToggleGroup` for single or multiple pressed-state choices, especially icon toolbars.
- Give icon-only toggles explicit accessible labels.

## Numeric And Value Controls

### `Slider`

- Preserve `Slider.Root`, `Track`, `Range`, and one or more `Thumb` parts.
- Use array values because sliders can support multiple thumbs.
- Use `onValueChange` for live updates and `onValueCommit` for commit-time effects.
- Reach for `minStepsBetweenThumbs` before hand-writing thumb-collision logic.
- Expect an `input` per thumb when the slider lives inside a form.

### `Select`

- Use when the chosen value belongs in form or app state but the menu should stay collapsed until opened.
- Prefer `RadioGroup` instead when the option set is short and staying visible improves usability.
- When controlling the rendered trigger value manually, keep the result accessible and consistent with the selected item.

### Specialized Auth Fields

- `One-Time Password Field` and `Password Toggle Field` are purpose-built input primitives.
- If a codebase already uses them, preserve the existing part structure and only abstract them further after inspecting the local implementation.

## Status And Feedback Inputs

### `Progress`

- Use for completion or loading progress, not for user input.
- Keep the actual progress value in app state and map it to the indicator transform or width in styles.
