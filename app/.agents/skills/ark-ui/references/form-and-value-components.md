# Form and Value Components

Use this reference for React Ark controls whose primary job is owning one value or one small cluster of values:

- `Checkbox`
- `Radio Group`
- `Switch`
- `Segment Group`
- `Toggle`
- `Toggle Group`
- `Rating Group`
- `Slider`
- `Angle Slider`
- `Number Input`
- `Pin Input`
- `Password Input`
- `Editable`
- `Timer`

Read [forms.md](./forms.md) alongside this file when submission, labels, validation, or `Field` context matters.

## Choice Controls

- `Checkbox`: boolean state or grouped multi-select values.
- `Radio Group`: exactly one value from a known set.
- `Switch`: boolean state with switch semantics.
- `Segment Group`: segmented single-choice UI.
- `Toggle`: one pressable on and off value.
- `Toggle Group`: grouped toggles with exclusive or multi-select behavior.
- `Rating Group`: scored selection such as stars or numeric ratings.

Prefer the smallest component that matches the semantics instead of reshaping one control into another.

## Range and Numeric Controls

- `Slider`: linear numeric range.
- `Angle Slider`: circular numeric range.
- `Number Input`: typed, stepped, and clamped numeric entry.

Keep display formatting separate from the stored numeric value. Preserve hidden inputs, labels, and value text when the component exposes them.

## Specialized Inputs

- `Pin Input`: segmented entry for OTP, invite codes, or verification codes.
- `Password Input`: password field with visibility affordances.
- `Editable`: inline read/edit switching for short content.
- `Timer`: timer state and control UI.

## Decision Rules

- Use `Pin Input` instead of splitting a text input into manual boxes.
- Use `Editable` only when view mode and edit mode are both first-class states.
- Use `Timer` when time progression itself is part of the component model, not just a formatted number on screen.

## Validation

- Confirm visible labels and field context still attach to the actual form control.
- Confirm controlled and uncontrolled value paths behave the same after refactors.
- Confirm keyboard behavior, stepping behavior, and disabled or read-only states still work.
