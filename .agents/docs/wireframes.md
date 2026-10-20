# Create and preview a wireframe

```sh
goddard-agent wireframe PATH.wireframe.json
```

Use wireframes to propose screen layouts, compare designs, and explain UI
states. They render as native elements using Goddard’s theme, fixed color
palette, and type scale.

Wireframes are static proposals. Buttons, inputs, charts, and other controls
are represented by labeled placeholders.

## Workflow

1. Read this format reference before writing your first wireframe.
2. Write a UTF-8 JSON file ending in `.wireframe.json` inside this task’s workspace.
3. Run `goddard-agent wireframe PATH.wireframe.json` with its workspace-relative path.
4. Fix any validation errors and run the command again.

Requires **Settings → Experiments → Wireframes**. The command validates the file
and requests a preview in a connected desktop client. For the selected task,
the right panel opens immediately. Background-task requests wait until that
task is selected.

A successful response confirms validation and broadcast of the request; it does
not confirm that a client displayed it. Requests missed while disconnected are
not replayed.

## Complete example

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
screen bounds is clipped.

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
matching stem: `plans/example.md` and `plans/example.wireframe.json`. Ordinary
task wireframes can use any workspace-relative location.
