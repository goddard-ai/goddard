# Keybinding Manager — GPUI Input Spike Decision Note

Date: 2026-09-17. Spike against pinned gpui fork `goddard-ai/zed@e36492a`
(`gpui = 0.2.2` in `Cargo.lock`).

## Answers

### 1. Runtime keymap replacement: YES, safe

`App::bind_keys()` mutates the live `Keymap` (`keymap.borrow_mut().add_bindings`)
and pushes `Effect::RefreshWindows` — bindings can be added at any time and take
effect on the next keystroke. `App::clear_key_bindings()` also exists, so a full
generated-map swap is `clear_key_bindings()` + `bind_keys(effective)` on the UI
thread, followed by re-registering any module bindings that are not catalog-owned.

Consequence: the spec's "a saved edit applies immediately" is a promise, not a
caveat. No app restart, no layer rebuild required. Registration order is
deterministic — `Keymap` keeps insertion order and later bindings win, so
precedence is controlled by emit order.

### 2. Physical key codes: NOT available in events — v1 is logical-only

`KeyDownEvent` carries only `{ keystroke: Keystroke, is_held,
prefer_character_input }`. `Keystroke` is `{ modifiers, key, key_char }` —
`key` is the logical ASCII-equivalent character, `key_char` the produced
character when they differ (option/IME). The native `keyCode` is read inside
`gpui_macos::events::parse_keystroke` but **discarded**; nothing physical
reaches the app layer.

Decision: **v1 stores and dispatches logical bindings only**, with the
per-binding `semantics: "logical"` field reserved for a future physical mode.
Physical dispatch would require patching the `goddard-ai/zed` fork to thread
`keyCode` through `Keystroke`/`KeyDownEvent` on all three platforms — feasible
(our own fork) but a separate ticket, not v1.

### 3. Multi-stroke chords: natively supported

`KeyBinding.keystrokes` is `SmallVec<[KeybindingKeystroke; 2]>` and the window
has a pending-keystroke state machine (`bindings_for_input`, `to_replay`).
Space-separated strokes in one binding string ("`ctrl-k ctrl-s`") just work —
the capture editor only needs to concatenate strokes, GPUI handles prefix
matching itself. Note: GPUI's dispatcher already defers prefix keystrokes, so
spec "prefix conflict" cases are about *discoverability*, not correctness.

### 4. Capture suppression: `intercept_keystrokes` exists

`App::intercept_keystrokes` registers observers that run before action dispatch
and can consume the event. The capture editor registers one while active,
commits strokes itself, and returns consume for everything except the emergency
Escape path. This is cleaner than fighting `KeyContext` stacking.

### 5. Layout detection: macOS YES via existing platform API

`Platform::keyboard_layout()` returns `PlatformKeyboardLayout { id, name }`
(backed by `TISCopyCurrentKeyboardLayoutInputSource`), and
`Platform::on_keyboard_layout_change(callback)` exists — the fork already
rebuilds `MacKeyboardMapper` on `NSTextInputContextKeyboardSelectionDidChange`.
Auto mode + change listener are free on macOS; Windows/Linux expose the same
trait surface (verification of which `id`s they return deferred to the layout
ticket).

### 6. IME / AltGr / dead keys

- `prefer_character_input` on `KeyDownEvent` marks AltGr-style events on
  Windows — capture must not commit those as Ctrl+Alt chords.
- `key_char` vs `key` handles dead-key/IME-produced characters; capture should
  commit on `key` (ASCII-equivalent) and reject events where `key` is empty or
  an IME placeholder.

## V1 semantic lock

- `semantics: "logical"` in all persisted records.
- Visualizer maps logical key → physical position for display only.
- macOS-only auto layout detection in v1; manual + lock elsewhere.
- Keypad distinction: `Keystroke` has no keypad flag — check whether
  `parse_keystroke` emits distinct `key` strings for numpad before promising
  numpad bindings; otherwise mark the limitation in the UI.

## Event fixtures

Captured during macOS validation of the manager (capture editor logs
`{modifiers, key, key_char, is_held, prefer_character_input}` for each event in
dev builds). Windows/X11/Wayland fixtures deferred to the platform hardening
ticket — the trait surface above is uniform, but per-OS `id()` strings and
AltGr behavior need real captures.
