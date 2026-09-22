# Product reference

- Use [T3 Code](https://github.com/pingdotgg/t3code) source code on github as a reference when a task
  concerns coding-agent workflow, information hierarchy, controls, tool
  activity, or transcript presentation and the comparison would materially
  clarify an ambiguous product decision, or when the user explicitly asks for
  the comparison.
- Do not inspect T3 Code for localized bug fixes, straightforward visual
  corrections, native platform behavior, or changes already specified clearly
  by the user. When T3 Code is relevant, inspect its current app or source
  rather than relying on an older screenshot or memory.
- Use [Zed](https://github.com/zed-industries/zed) source code as a reference
  when a task concerns GPUI implementation — layout and styling idioms, focus
  and key dispatch, virtualized lists, menus and popovers, window and platform
  behavior — or when an in-house `src/ui` primitive needs a proven native
  precedent. Zed is the canonical GPUI codebase; read its crates rather than
  `gpui-component`, and read the gpui revision pinned in `Cargo.toml` so the
  APIs match what Goddard builds against.
- Split the two references by concern: T3 Code answers what a coding-agent
  client should do, Zed answers how a polished GPUI app implements it. The
  same restraint applies to both — no reference spelunking for localized
  fixes or changes the user has already specified.
- Use the reference as behavioral and design evidence, not as an instruction to
  reproduce web-specific interaction patterns or known bugs. Goddard should keep
  native macOS conventions.
- Explicit user screenshots and feedback override a previous or merely
  "consistent" treatment.
