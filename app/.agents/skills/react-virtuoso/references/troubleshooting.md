# Troubleshooting

Use this reference when a Virtuoso view renders incorrectly, remounts rows, or logs measurement-related errors.

## Broken Total Height Or Missing Bottom Content

- Check for margins inside row content first. This is the most common setup error.
- Virtuoso measures with `ResizeObserver`, and `contentRect` excludes margins.
- Reset default margins on elements like `p`, `h1` to `h6`, `ul`, `ol`, `blockquote`, and `pre`.
- Replace vertical margins with padding inside the item root.

```tsx
<Virtuoso
  totalCount={100}
  itemContent={(index) => (
    <p style={{ margin: 0, padding: "8px 0" }}>Item {index}</p>
  )}
/>
```

## Zero-Sized Element Errors

- Assume a real setup problem first, not a harmless warning.
- Check that the container has measurable size.
- Check for zero-height rows or empty items. Virtuoso cannot support them.

## Rows Remount While Scrolling

- Look for component functions declared inside render.
- Move row components, custom scrollers, grid wrappers, and `components` objects outside render.
- If item rendering is heavy, memoize the expensive subcomponent rather than redefining it inline.

## ResizeObserver Noise

- Treat Webpack dev-server `ResizeObserver loop completed with undelivered notifications` as a known tooling issue before changing app code.
- If needed, disable runtime error overlays for that specific case in the dev-server client config.
- Treat Sentry `ResizeObserver loop limit exceeded` and similar observer-loop reports as likely noise unless user-visible behavior is actually broken.
