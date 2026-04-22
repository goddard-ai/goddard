# Control Flow and Styles

Source: derived from `https://tsrx.dev/llms.txt` fetched on April 22, 2026.

## `if` / `else if` / `else`

Use normal JavaScript conditionals inside templates.

```tsrx
component StatusBadge({ status }: { status: string }) {
  <div>
    if (status === "active") {
      <span class="badge active">{"Online"}</span>
    } else if (status === "idle") {
      <span class="badge idle">{"Away"}</span>
    } else {
      <span class="badge">{"Offline"}</span>
    }
  </div>
}
```

## `for ... of` with `index` and `key`

Use `for ... of` directly in the template. Add `index name` for the loop index and `key expr` for stable identity.

```tsrx
component TodoList({ items }: { items: Todo[] }) {
  <ul>
    for (const item of items; index i; key item.id) {
      <li>{`${i + 1}. ${item.text}`}</li>
    }
  </ul>
}
```

Prefer `key expr` whenever item identity matters across reorders or incremental updates.

## `switch`

Use standard `switch` statements for multi-branch rendering. `break` terminates a case and fall-through still works.

```tsrx
switch (status) {
  case "loading":
    <p>{"Loading..."}</p>
    break;
  case "success":
    <p class="success">{"Done!"}</p>
    break;
  default:
    <p>{"Unknown status."}</p>
}
```

## `try` / `pending` / `catch`

Use `try { ... } catch (e) { ... }` for error fallback UI. Add `pending { ... }` to model async loading boundaries.

```tsrx
const UserProfile = lazy(() => import("./UserProfile.tsrx"));

export component App() {
  try {
    <UserProfile id={1} />
  } pending {
    <p>{"Loading..."}</p>
  } catch (e) {
    <p>{"Something went wrong."}</p>
  }
}
```

Treat `try`, `pending`, and `catch` as template control-flow constructs, not as ad hoc wrapper components.

## Early Returns

Use a bare `return;` for guard clauses after rendering fallback content. Do not return a value.

```tsrx
component Dashboard({ user }: { user: User | null }) {
  if (!user) {
    <p>{"Please sign in."}</p>
    return;
  }

  <h1>{`Welcome, ${user.name}`}</h1>
}
```

Do not translate React-style `return (...)` directly into TSRX.

## Scoped Styles

Use a component-local `<style>` block to scope selectors to the component with a unique hash.

```tsrx
component Card() {
  <div class="card">
    <h2>{"Scoped title"}</h2>
  </div>

  <style>
    .card {
      padding: 1.5rem;
      border: 1px solid #ddd;
    }

    h2 {
      color: #333;
    }
  </style>
}
```

Treat scoped styles as the default. Parent styles do not automatically leak into child components.

## `:global(...)`

Use `:global(...)` to opt out of scoping for resets or selectors that must escape the component boundary.

Examples called out by the source:

- `:global(.class-name)`
- `:global(.foo, .bar)`
- `.scoped :global(.unscoped) .also-scoped`

Keep `:global(...)` intentional and narrow.

## `#style`

Use `#style.className` to pass a scoped class name to a child component.

```tsrx
component Badge({ className }: { className?: string }) {
  <span class={`badge ${className ?? ""}`}>{"New"}</span>
  <style>
    .badge {
      padding: 0.25rem 0.5rem;
    }
  </style>
}

component App() {
  <Badge className={#style.highlight} />
  <style>
    .highlight {
      background: #e8f5e9;
      color: #2e7d32;
    }
  </style>
}
```

The referenced class must appear as a standalone selector in the surrounding component's `<style>`.
