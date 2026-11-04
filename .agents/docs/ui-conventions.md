# UI conventions

## Inspector source locations

- "Goddard: Inspect Elements" labels the picked element with the `file:line`
  of its *construction* site: gpui captures `Location::caller()` inside
  `div()`/`svg()`/`img()`/`uniform_list()`, and `#[track_caller]` only
  propagates through functions that are themselves marked.
- Mark `#[track_caller]` on any function whose return value is an element (or
  an element-bearing component, like `MenuChip::new`) that a caller drops into
  its tree, so the label reports the call site instead of a line inside the
  helper. `crates/waku-ui/src/ui/` constructors follow this; `render_*` helpers in `src/app/`
  may opt in the same way when the call site is the identifying location.
- Do not mark `Render`/`RenderOnce::render` implementations — the attribute
  would point every element built inside at gpui's `ViewElement` internals
  rather than the component's own lines. Helpers whose bodies render distinct
  branches per call (e.g. `menu::render_menu_item`) are also better left
  unmarked: their inner construction lines are the informative answer.
- Only `Interactivity`-backed elements (div, svg, img, uniform_list) register
  inspector hitboxes; `canvas`, `deferred`, `list`, and view boundaries never
  surface a location, so marking a function that only builds those has no
  effect. A pick resolves to the topmost pickable element under the cursor.

## Rounded corners clip nothing

- GPUI's `overflow_hidden` clips descendants to a rectangle, not the parent's
  rounded corners. Give child backgrounds, hover overlays, and images that
  reach a rounded edge their own matching corner radii, accounting for the
  parent's border inset. Parent rounding alone does not clip child paint.

## Provider content in the transcript

- For provider-native content such as citations, reasoning, and tool events,
  verify the real provider payload and preserve its ordering. Never expose
  private provider control markers in the transcript.

## Screen headers

- Resolve the main column once through `MainScreen` in `src/app/render.rs`.
  Use that same owner for body and header rendering. Add new screens to the
  enum and its exhaustive matches; never infer a page's identity from the
  remembered selected task.
- Every screen owns its title, badges, and actions. Inbox, Drafts, Automations,
  Projects, and Brain pass their header content to `render_header_frame` inside
  the page's keyboard/action context. Do not stack a task header above them.
- `render_header_frame` owns window controls, drag clearance, and the shared
  sidebar toggle, unread bell, and history arrows. With the sidebar open, its
  title bar owns those navigation buttons; with it collapsed, the frame does.
  Unavailable arrows stay visible and disabled. Settings uses its dedicated
  layout without the shared navigation buttons.
- Task details belong only to task/chat surfaces (including their VoicePad).
  Standalone terminals and deliverable previews use their own identity.
- When adding a screen, check it after selecting a titled task, with the
  sidebar open and collapsed. Confirm a single header, the screen's identity
  and actions, accessible navigation, and working page shortcuts.
