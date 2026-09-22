# Accessibility

- Treat accessibility as a product requirement too. GPUI does not yet expose a
  screen-reader tree, so here it means keyboard operability, honored system
  settings, and legibility — none of which depend on that missing API, and all
  of which regress silently if left unchecked.
- Every control reachable by mouse must be reachable and operable by keyboard.
  Use `track_focus` with `tab_index`, `tab_group`, and `tab_stop`, give focus a
  visible treatment via `focus_visible`, and support the conventional keys for
  the widget (arrows, `home`/`end`, `enter`/`space`, `escape`).
- Honor the system's reduce-motion setting. `with_animation` already respects
  `App::reduce_motion`, but a direct `window.request_animation_frame` for
  decorative motion must check `cx.reduce_motion()` and skip the request.
- Never encode meaning in color, hover, or motion alone. Pair a status color
  with an icon or text, and make sure anything revealed on hover is also
  reachable by keyboard focus.
- Keep text and icons legible against their surface in both themes, and give
  interactive targets enough hit area — extend the hit region rather than
  shrinking to the glyph.
