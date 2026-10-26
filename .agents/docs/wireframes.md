# Create and preview a wireframe

```sh
goddard-agent wireframe PATH.wireframe.html
```

Use wireframes to propose screen layouts, compare designs, and explain UI
states. Prefer self-contained HTML/CSS on macOS and Windows. The preview uses
Goddard's active theme and updates when the theme changes. Existing JSON
wireframes still render as native elements on all desktop platforms.

Wireframes are static proposals. Their controls illustrate a design; they do
not navigate or run application code. Comment controls belong to Goddard and
remain separate from the authored design.

## Workflow

1. Read this format reference before writing your first wireframe.
2. Write a UTF-8 `.wireframe.html` file inside this task's workspace. Use
   `.wireframe.json` when a native sketch or Linux preview is required.
3. Run `goddard-agent wireframe PATH` with its workspace-relative path.
4. Fix any validation errors and run the command again.

Requires **Settings → Experiments → Wireframes**. The command validates the file
and requests a preview in a connected desktop client. For the selected task,
the right panel opens immediately. Background-task requests wait until that
task is selected.

A successful response confirms validation and broadcast of the request; it does
not confirm that a client displayed it. Requests missed while disconnected are
not replayed.

## HTML example

Save this as `designs/tasks.wireframe.html`:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="goddard-wireframe-version" content="1">
  <meta name="goddard-wireframe-id" content="task-review">
  <title>Task review proposal</title>
  <style>
    section { display: flex; gap: 16px; padding: 24px; }
    aside { width: 200px; padding: 16px; background: var(--wf-surface); }
    main { flex: 1; }
  </style>
</head>
<body>
  <section data-wireframe-screen="tasks" aria-label="Tasks populated"
           data-wireframe-width="800" data-wireframe-height="600">
    <aside data-wireframe-id="sidebar" aria-label="Task navigation">Tasks</aside>
    <main data-wireframe-id="detail"><h1>Review onboarding</h1><p>Ready for review</p></main>
  </section>
</body>
</html>
```

Run `goddard-agent wireframe designs/tasks.wireframe.html`. The right panel
opens the themed preview. Use its screen selector to switch between named
screens and its source toggle to inspect or edit HTML. Linux shows a clear
HTML-preview limitation; source remains available.

The version and document ID meta elements must appear exactly once in
`head`. Preserve the document ID across revisions. A separate proposal copied
from another file needs a new document ID. Discussion is scoped to the owning
task; files cannot impersonate a comment's author.

Each screen needs a unique `data-wireframe-screen` ID. Its `aria-label` supplies
the visible name. Dimensions default to 800 × 600 and can be set with
`data-wireframe-width` and `data-wireframe-height`; each must be finite,
positive, and at most 16,384 pixels. Screens shrink to fit the panel, without
upscaling. Only the selected screen is displayed.

Give meaningful elements stable `data-wireframe-id` values and useful
`aria-label` descriptions. Anchor IDs must be unique within their screen;
the same ID may appear in different screens. All IDs contain 1–128 ASCII
letters, digits, dots, underscores, or hyphens. Preserve IDs for surviving
concepts when revising a design. Screens cannot be nested, and anchors must
belong to a screen.

### Active theme

Use these variables for ordinary UI colors instead of hardcoded colors:

| Variable | Goddard role |
| --- | --- |
| `--wf-canvas` | Canvas background |
| `--wf-surface` | Surface background |
| `--wf-inset` | Recessed background |
| `--wf-text` | Primary text |
| `--wf-secondary` | Secondary text |
| `--wf-border` | Subtle border |
| `--wf-info` | Informational blue; matches unread markers and comment dots |
| `--wf-accent` | Current theme's accent |
| `--wf-color-scheme` | Active light/dark scheme for native HTML controls |

The supplied base stylesheet provides typography, basic form-control styling,
focus outlines, and reduced-motion behavior. Standard CSS layout, including
flex and grid, remains available. Keep styles inline in the document. Scripts,
event-handler attributes, embedded frames, templates, imports, external assets,
file access, navigation, downloads, and form submission are unavailable.

### Agent comments and user replies

Attach a comment after writing the document:

```sh
goddard-agent wireframe comment add designs/tasks.wireframe.html \
  --screen tasks --anchor sidebar --idempotency-key sidebar-rationale-v1 \
  --text 'Keep task navigation visible while reviewing a thread.'
```

Omit `--anchor` for a screen-level comment. Use comments selectively to explain
a consequential decision, tradeoff, or question. Preserve the idempotency key
when retrying the same operation; a different comment needs a different key.
`--file PATH` or `--file -` can replace `--text` for longer or multiline text.

The user clicks the themed dot to open the comment and reply input. Reading
clears only the unread cue: the marker remains. Users can save replies, add
their own threads through **Add comment**, and resolve or reopen threads in
the comments list. The element picker provides a keyboard path to commenting;
unanchored content can receive a screen-level thread.

Saved discussion lives in the owning daemon's database, independently of the
HTML. Regenerating a design or sending feedback never drains its threads.
Removed anchors retain their threads in the comments list as **Anchor removed**;
restoring the same ID restores an open thread's marker. Save source edits
before attaching comments to new anchors.

Read discussion with:

```sh
goddard-agent wireframe comment list designs/tasks.wireframe.html
```

The response includes thread IDs and `nextOffset`; pass the returned offset
with `--offset` to read another page. To inspect or answer a particular thread,
replace `THREAD_UUID` below with its returned ID:

```sh
goddard-agent wireframe comment read designs/tasks.wireframe.html --thread THREAD_UUID
goddard-agent wireframe comment reply designs/tasks.wireframe.html \
  --thread THREAD_UUID --idempotency-key sidebar-answer-v1 --text 'Agreed.'
```

Saving or reading a reply does not start an agent turn. **Add to message**
stages the saved thread, document path, revision, and anchor in the owning
task's composer. The user may edit or remove that text and chooses when to
Send. Agents can also read saved replies through the commands above.

HTML files are limited to 1 MiB, 2,000 DOM nodes, 64 nesting levels, and 64
screens. Each document's discussion supports at most 200 threads, 128 messages
per thread, and 1 MiB of stored discussion data. Each message accepts at most
16,384 UTF-8 bytes. Validation or save errors remain visible without discarding
the user's reply draft.

## Legacy JSON example

Save this as `designs/tasks.wireframe.json`:

```json
{
  "version": 1,
  "name": "Task list proposal",
  "screens": [
    {
      "name": "Tasks — populated",
      "width": 800,
      "height": 600,
      "root": {
        "type": "frame",
        "direction": "column",
        "gap": 16,
        "padding": 24,
        "children": [
          {
            "type": "frame",
            "direction": "row",
            "align": "center",
            "gap": 12,
            "children": [
              {
                "type": "text",
                "text": "Tasks",
                "role": "title",
                "width": "fill"
              },
              {
                "type": "rect",
                "label": "New task",
                "width": 120,
                "height": 36,
                "fill": "accent",
                "cornerRadius": 6,
                "annotations": "Opens the task composer."
              }
            ]
          },
          {
            "type": "divider"
          },
          {
            "type": "frame",
            "direction": "column",
            "gap": 8,
            "padding": 16,
            "fill": "surface",
            "cornerRadius": 8,
            "children": [
              {
                "type": "text",
                "text": "Review onboarding",
                "role": "label"
              },
              {
                "type": "text",
                "text": "Ready for review",
                "role": "caption",
                "tone": "secondary"
              }
            ]
          }
        ]
      }
    }
  ]
}
```

Then run:

```sh
goddard-agent wireframe designs/tasks.wireframe.json
```

## Document and screens

| Field | Required | Meaning |
| --- | --- | --- |
| `version` | Yes | Must be the integer `1`. |
| `name` | Yes | Document name. |
| `screens` | No | Array of screens; defaults to `[]`. Include at least one for a useful preview. |
| `screens[].name` | Yes | Screen name; use it to distinguish layouts or states. |
| `screens[].width` | Yes | Positive screen width in pixels. |
| `screens[].height` | Yes | Positive screen height in pixels. |
| `screens[].root` | Yes | Root node, usually a `frame`. |

The root fills its screen; its own sizing is ignored. Content outside the
screen bounds is clipped. Previews shrink uniformly to fit the right panel’s
available width, preserving the layout and aspect ratio. Smaller screens stay
at their declared size; tall screens remain vertically scrollable.

## Node types

Every node requires a `type`: `frame`, `rect`, `text`, or `divider`.
Only `frame` can contain children.

### frame

Arranges children and can provide a styled container.

| Field | Default | Values or meaning |
| --- | --- | --- |
| `direction` | `"column"` | `"column"` or `"row"`. |
| `gap` | `0` | Pixel spacing between children. |
| `padding` | `0` | One pixel number, or `{ "x": 16, "y": 8 }`. Omitted axes default to `0`. |
| `align` | `"stretch"` | Cross-axis alignment: `"start"`, `"center"`, `"end"`, `"stretch"`. |
| `justify` | `"start"` | Main-axis alignment: `"start"`, `"center"`, `"end"`, `"space_between"`. |
| `width`, `height` | `"hug"` | Pixel number, `"hug"`, or `"fill"`. |
| `fill`, `stroke` | None | Surface tone from the palette below. |
| `cornerRadius` | `0` | Radius in pixels. |
| `annotations` | None | String explaining intent; shown as a tooltip. |
| `children` | `[]` | Ordered array of child nodes. |

### rect

Represents a button, input, image, avatar, chart, or other placeholder.

| Field | Default | Meaning |
| --- | --- | --- |
| `label` | None | Text naming the placeholder. |
| `width`, `height` | `"hug"` | Same sizing rules as frames. |
| `fill`, `stroke` | None | Surface tone. |
| `cornerRadius` | `0` | Radius in pixels. |
| `annotations` | None | Intent tooltip. |

Give unlabeled rectangles explicit dimensions so they occupy visible space.

### text

Displays one line of text.

| Field | Default | Values or meaning |
| --- | --- | --- |
| `text` | Required | Displayed string. |
| `role` | `"body"` | `"title"`, `"body"`, `"caption"`, `"label"`. |
| `tone` | `"primary"` | `"primary"`, `"secondary"`, `"tertiary"`, `"accent"`. |
| `align` | `"start"` | Horizontal text alignment: `"start"`, `"center"`, `"end"`. |
| `width`, `height` | `"hug"` | Same sizing rules as frames. |
| `annotations` | None | Intent tooltip. |

Text truncates with an ellipsis when it exceeds its box. Use separate text nodes
for separate lines.

### divider

Accepts only an optional `annotations` string. It becomes a horizontal rule
inside a column and a vertical rule inside a row, spanning the parent’s inner
cross axis.

## Sizing and layout

- A number means a fixed pixel size: `"width": 240`.
- `"hug"` sizes to content.
- `"fill"` shares remaining main-axis space equally with other fill siblings.
  On the cross axis, it fills the parent’s inner span.

A row’s main axis is horizontal; a column’s is vertical. `justify` controls the
main axis, while `align` controls the cross axis. The default `align: "stretch"`
stretches children along the cross axis.

Use nested frames for page sections, toolbars, sidebars, cards, and rows.

## Surface palette

Use these tokens for `fill` and `stroke`:

| Token | Intended use |
| --- | --- |
| `"surface"` | Raised cards and selected surfaces. |
| `"inset"` | Recessed wells and input placeholders. |
| `"overlay"` | Mid-emphasis rows and placeholders. |
| `"ink"` | Dark emphasis and primary buttons. |
| `"accent"` | Interactive emphasis and unread cues. |

Use the fixed tokens rather than hex colors, font names, CSS, or HTML.

## Authoring guidance and limits

Keep each screen focused on one layout or state. Add separate named screens for
meaningful alternatives, empty states, loading states, and errors. Use realistic
labels and annotations to explain behavior that the static layout cannot
demonstrate.

Files must stay inside the task’s workspace, including through symlinks. The
command accepts regular files up to **1 MiB**. Documents support at most
**2,000 nodes** across all screens, and screen dimensions must be finite and
greater than zero.

For a planning-session companion, place the wireframe beside its plan with the
matching stem: `plans/example.md` and `plans/example.wireframe.html`, or the
legacy `plans/example.wireframe.json`. HTML takes precedence when both exist;
invalid HTML is reported rather than silently falling back to JSON. The plan
preview names the selected HTML companion. Ordinary task wireframes can use
any workspace-relative location.
