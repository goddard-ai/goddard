# Utilities and Runtime

Use this reference for React Ark utilities that are not one full component family but still carry important behavior:

- `Client Only`
- `Download Trigger`
- `Environment`
- `Focus Trap`
- `Format Byte`
- `Format Time`
- `Format Relative Time`
- `Frame`
- `Highlight`
- `JSON Tree View`
- `Locale`
- `Swap`

Read [animation-and-presence.md](./animation-and-presence.md) separately for `Presence`.

## Client and DOM Environment Helpers

- `Client Only`: render children only on the client, optionally with a fallback.
- `EnvironmentProvider`: point Ark at the right `document` or root in iframe, Shadow DOM, or Electron-like environments.
- `Focus Trap`: trap keyboard focus in a region when the component family does not already own that behavior.
- `Frame`: render React content in an iframe and use lifecycle hooks for injected head content or mount work.

Use these instead of custom environment bookkeeping when Ark is already part of the stack.

## Download and Formatting

- `Download Trigger`: trigger file downloads from text, blobs, or similar data by passing `data`, `fileName`, and `mimeType`.
- `Format.Byte`: format file sizes or transfer sizes.
- `Format.Time`: format `HH:mm[:ss]` strings or `Date` values.
- `Format.RelativeTime`: format dates via relative-time semantics.

Prefer Ark format helpers when the repo already depends on Ark and the formatting requirement is local to the UI.

## Text and Data Display

- `Highlight`: render matched substrings for search or filter results.
- `JSON Tree View`: inspect arbitrary JSON data as a tree.
- `Swap`: switch between two indicators, usually `on` and `off`, without hand-rolled DOM swapping.

Use `Highlight` with searchable collections such as `Combobox`. Use `JSON Tree View` for inspection UIs, not as a replacement for domain-specific tree components.

## Locale

Use locale-aware Ark helpers when filtering, formatting, or date presentation depends on the user locale.

- Pass `locale` and `startOfWeek` to components such as `Date Picker` when calendar rendering should localize.
- Prefer locale-aware helpers over manual case folding or naive string comparison for filtered collections.

## Validation

- Confirm SSR or client-only boundaries still behave after hydration.
- Confirm focus trapping does not conflict with a component that already traps focus.
- Confirm downloaded files have the right name and MIME type.
- Confirm localized formatting, filtering, and week starts match product expectations.
