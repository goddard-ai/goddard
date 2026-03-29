---
name: pretext
description: Build, refactor, debug, and review browser UI code that uses Pretext for DOM-free multiline text measurement and manual text layout via `prepare`, `layout`, `prepareWithSegments`, `layoutWithLines`, `walkLineRanges`, `layoutNextLine`, `clearCache`, or `setLocale`. Use when Codex must predict text height without DOM reads, preserve line count while shrinkwrapping, route text around shapes or varying-width slots, hand text across columns, or compose measured inline fragments in JS/TS.
---

# pretext

Use Pretext when text geometry needs to be computed in JavaScript instead of read from the DOM. Keep the font shorthand and line height synced with the eventual rendered styles, prepare text once, and treat repeated layout calls as the cheap hot path.

## Start Here

- Identify which mode the task needs before editing:
  - paragraph metrics only: `prepare()` + `layout()`
  - manual line control: `prepareWithSegments()` + `layoutWithLines()`, `walkLineRanges()`, or `layoutNextLine()`
- Reuse prepared text across resizes, drag events, and repeated renders. Re-preparing on every frame defeats the main performance win.
- Match the `font` string to the real rendered text. Use the same canvas-style shorthand the UI will actually render with.
- Match the `lineHeight` argument to the actual CSS or manual line spacing.
- Wait for web fonts before trusting measurements as final. In browser demos, `document.fonts.ready` is the safe sync point.
- Prefer named fonts over `system-ui` when accuracy matters on macOS.

## Choose The API

- Use `prepare(text, font)` and `layout(prepared, maxWidth, lineHeight)` when only `height` and `lineCount` matter.
  - Reach for this for virtualization, masonry, accordions, and overflow prediction.
  - See [accordion.ts](./demos/accordion.ts) and [masonry/index.ts](./demos/masonry/index.ts).
- Use `prepareWithSegments(text, font)` and `layoutWithLines(prepared, maxWidth, lineHeight)` when every line has the same width and you need the actual line strings.
  - Reach for this for canvas, SVG, or absolutely positioned DOM lines.
- Use `walkLineRanges(prepared, maxWidth, onLine)` when you need widths and start/end cursors without allocating strings.
  - Reach for this for shrinkwrap searches, balanced-width experiments, width measurement, or inline-fragment bookkeeping.
  - See [bubbles-shared.ts](./demos/bubbles-shared.ts), [dynamic-layout.ts](./demos/dynamic-layout.ts), and [rich-note.ts](./demos/rich-note.ts).
- Use `layoutNextLine(prepared, cursor, maxWidth)` when the available width changes from one line to the next.
  - Reach for this for obstacle-aware flow, flowing around logos, variable-width slots, and multi-column handoff.
  - See [dynamic-layout.ts](./demos/dynamic-layout.ts), [editorial-engine.ts](./demos/editorial-engine.ts), and [rich-note.ts](./demos/rich-note.ts).
- Pass `{ whiteSpace: 'pre-wrap' }` to `prepare()` or `prepareWithSegments()` when spaces, tabs, and hard line breaks must remain visible like a textarea.
- Use `setLocale(locale)` before future `prepare()` calls when line breaking must follow a specific locale. Remember that it also clears shared caches.
- Use `clearCache()` only when releasing accumulated shared measurement caches is worth the recompute cost.

## Workflow

- Confirm whether the task needs exact paragraph metrics or explicit line geometry.
- Cache prepared text by the inputs that affect measurement: text, font, and any whitespace mode or locale assumptions.
- Keep measurement and layout in JavaScript. Do not reintroduce hot-path DOM reads such as `getBoundingClientRect()` just to recover information Pretext already provides.
- If the task only needs height, stop at `layout()`. Do not switch to segmented APIs unless line strings or cursors are materially required.
- If the task mixes text with non-text chrome such as chips, pills, code frames, or padding, prepare each text fragment separately and account for non-text width yourself.
- If the task routes around shapes, compute blocked horizontal intervals for each line band, carve the remaining slots, and feed those widths to `layoutNextLine()`.
- If the task hands text across columns or regions, carry the returned `end` cursor forward instead of splitting the source text manually.

## Demo Map

- [accordion.ts](./demos/accordion.ts) and [accordion.html](./demos/accordion.html): compute expandable panel heights with `layout()` instead of hidden DOM measurement.
- [masonry/index.ts](./demos/masonry/index.ts) and [masonry/index.html](./demos/masonry/index.html): predict card heights ahead of render for a masonry-style layout.
- [bubbles-shared.ts](./demos/bubbles-shared.ts) and [bubbles.ts](./demos/bubbles.ts): preserve line count while binary-searching the tightest multiline bubble width.
- [dynamic-layout.ts](./demos/dynamic-layout.ts) and [wrap-geometry.ts](./demos/wrap-geometry.ts): route text around polygonal obstacles, derive line slots per band, and continue one text stream across columns.
- [editorial-engine.ts](./demos/editorial-engine.ts): reflow text around moving obstacles with repeated `layoutNextLine()` calls in a live interactive layout.
- [rich-note.ts](./demos/rich-note.ts) and [rich-note.html](./demos/rich-note.html): lay out mixed inline fragments where chips stay atomic while text fragments wrap naturally.
- [variable-typographic-ascii.ts](./demos/variable-typographic-ascii.ts): use `prepareWithSegments()` as a glyph width measurement primitive, not only as paragraph layout.

## Default Assumptions And Caveats

- Assume `white-space: normal`, `word-break: normal`, `overflow-wrap: break-word`, and `line-break: auto` unless the caller explicitly opts into `pre-wrap`.
- Expect very narrow widths to break inside words at grapheme boundaries because `overflow-wrap: break-word` is part of the target behavior.
- Treat prepared states as immutable snapshots. If the font, locale, or whitespace behavior changes, prepare again instead of mutating cached state.
- Use Pretext for JS-driven layout or performance-sensitive measurement. If plain CSS already solves the task and no line geometry is needed, do not add Pretext just to reproduce default browser flow.
