# App Form Patterns

Use this reference for app-local form guidance that intentionally does not live in `app/AGENTS.md`.

For bootstrap rules and broad app contribution policy, follow `app/AGENTS.md`, `references/app-contributing.md`, and `references/app-best-practices.md` first.

## When To Use A Form Model

- Use one feature-local form model when a form has cross-field derivation, reset semantics, async option loading, or state that is shared across a dialog boundary.
- Use simpler local state for trivial forms with a few independent inputs and no meaningful derived state.
- Keep the form model local to the feature unless multiple forms prove they need the same behavior.

## State Shape

- Keep one mutable source of truth for draft field values.
- Put canonical derived state in `computed(...)` values on the form model instead of recalculating it across render sites.
- Prefer semantic derived values such as `canSubmit`, `selectedAdapter`, or a fully assembled request payload over ad hoc booleans and inline request construction.
- Keep normalization logic next to the state it normalizes. If one field depends on another async field catalog, reconcile that selection inside the model instead of scattering watch-and-sync logic through components.

## Component Boundaries

- Pass the form object through the component boundary instead of threading a bag of values and change callbacks through the tree.
- Keep dialog lifecycle outside the form body. Dialog open, close, and reset-on-open belong to the dialog wrapper, while the form component should focus on fields and submission.
- Keep async field loading local to the field or narrow subcomponent that needs it, not the whole dialog or page shell.
- When a query-backed subfield needs to publish data into the form model, keep that write narrow and intentional.

## Async Fields And Suspense

- Suspend narrowly around the async field that actually needs data, not around the whole form.
- Use a fallback that preserves the same field footprint when possible, such as a disabled placeholder control, so the form layout stays stable while data resolves.
- Keep field labels and surrounding layout outside the suspended subcomponent unless the entire field truly depends on the async read.

## Submission

- Prefer one canonical computed payload on the form model for submit actions instead of rebuilding request objects inline in every submit callback.
- Let the submit handler read the computed payload, perform the mutation, and handle post-submit effects such as closing a dialog, resetting the form, or opening a tab.
- Do not mirror query loading or mutation status into extra form state unless the UI genuinely needs an additional app-specific state machine.

## Readability

- Keep form JSX visually segmented by major parts so the structure is easy to scan.
- Extract narrow subcomponents for clearly bounded responsibilities such as one async select field, not for arbitrary chunks of JSX that only make the form harder to follow.
- Prefer names that describe the field or responsibility directly, such as `AdapterSelect`, over generic helper names like `Fields` or `Controls`.
