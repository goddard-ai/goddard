---
name: tsrx-react
description: Build, refactor, debug, review, and explain React `.tsrx` code written with TSRX. Use when Codex must translate JSX or TSX into TSRX syntax, preserve statement-based JSX and lexical template scoping, work with lazy destructuring or refs, edit scoped style blocks, or handle template control flow such as `if`, `for`, `switch`, and `try` / `pending` / `catch`.
---

## Overview

TSRX is a TypeScript language extension for authoring React UI in `.tsrx` files.

Key traits:

- Treat JSX elements as statements rather than expressions.
- Keep control flow directly in the template with `if`, `for`, `switch`, and `try`.
- Keep locals scoped near the JSX that uses them.
- Support lazy destructuring with `&{ ... }` and `&[ ... ]`.
- Scope component styles automatically with hashed selectors.
- Preserve TypeScript types through the compile step.

## Mental Model

- Treat `.tsrx` as a React authoring language, not as plain JSX with a few helpers.
- Keep markup, control flow, local declarations, and style blocks close together in the component body.
- Prefer local clarity over JSX habits carried over from function components.
- Let TSRX features such as lexical scoping, lazy destructuring, and statement-position JSX do the structural work instead of recreating JSX-era patterns.

## Start Here

- Inspect the current `.tsrx` authoring patterns first: component shape, control flow, refs, styles, and any use of top-level `await`.
- Use the in-file sections below as needed:
  - Read `Components and Expression Rules` before editing component declarations, text, props, refs, children, or `<tsx>` expression-position JSX.
  - Read `Control Flow and Styles` before editing `if`, `for`, `switch`, `try` / `pending` / `catch`, early returns, or scoped styles.
  - Read `React Behavior and Common Patterns` before relying on React hook lifting, top-level `await`, or common escape-hatch patterns.
- Preserve the existing React semantics before abstracting anything.

## Workflow

1. Identify the current React authoring pattern and its constraints.
   - Check how components express guards, loops, nested scopes, and refs.
   - Check whether components rely on top-level component-body `await`.
   - Check how local style blocks and passed-through `#style` classes are used.
2. Apply TSRX syntax instead of JSX habits.
   - Declare UI building blocks with `component` or `export component`.
   - Place JSX elements directly in the component body as statements.
   - Wrap text and inline values in `{...}`. Bare text is invalid.
   - Use `<tsx>...</tsx>` only when JSX must appear in expression position, and keep its contents to standard TSX or JSX rather than TSRX-only template syntax.
   - Use a bare `return;` only to stop later template output after rendering a guard branch.
3. Use TSRX-specific features deliberately.
   - Use `&{ ... }` or `&[ ... ]` lazy destructuring when deferred property or index access is clearer than repeated lookups.
   - Use `{ref variable}` or `{ref callback}` instead of inventing custom React ref plumbing.
   - Keep local declarations next to the JSX they feed; nested element bodies create lexical scopes.
   - Use `children={expr}` when children is computed rather than nested JSX.
4. Preserve React behavior and safety.
   - Do not `return <JSX />`, `return someValue`, or assign bare JSX to variables outside `<tsx>`.
   - Do not mark components `async`.
   - Use top-level component-body `await` only when the surrounding React app expects it.
   - Do not use `for await...of` inside React component templates.
   - Do not pass scoped classes across component boundaries except through `#style.className`.
5. Validate the generated behavior after editing.
   - Check that keyed loops, refs, and lazy destructuring still behave correctly in the generated React code.
   - Check that scoped styles stay local and that any `:global(...)` selector is intentional.
   - Check that guard clauses and async boundaries still lower to the intended React behavior.

## Components and Expression Rules

### Components

Declare components with `component`, not `function`. Keep the template directly in the component body and do not return JSX.

```tsrx
export component Button({ label, onClick }: {
  label: string;
  onClick: () => void;
}) {
  <button class="btn" {onClick}>
    {label}
  </button>
}
```

Prefer these rules:

- Use `component Name(props: Props) { ... }` or `export component Name(...) { ... }`.
- Keep JSX statements, handlers, and `<style>` in the component body.
- Do not mix `function Component()` and `component Component()` styles in the same TSRX code.
- Do not `return <JSX />` from a component body.

### Statement-Based JSX

Treat JSX as statements, not expressions. Put text inside expression containers.

```tsrx
component Greeting() {
  <h1>{'Hello World'}</h1>
  <p>{`Count: ${count}`}</p>
}
```

Do not write bare text:

```tsrx
<div>Hello World</div>
```

Write this instead:

```tsrx
<div>{'Hello World'}</div>
```

### `<tsx>` for Expression Position

Wrap JSX in `<tsx>...</tsx>` when JSX must live in expression position, such as assignment, helper returns, or prop values.

```tsrx
component App() {
  const title = <tsx><span class="title">{'Settings'}</span></tsx>;
  <Card {title} />
}
```

Use `<tsx>` when:

- Assigning JSX to a variable
- Returning JSX from a plain helper function
- Passing JSX through a prop value

Inside `<tsx>`, write ordinary TSX or JSX only. `<tsx>` is an expression-position escape hatch, not a nested TSRX template, so TSRX-only syntax such as template `if` blocks, `for ... of` loops with `index` or `key`, `switch`, `try` / `pending` / `catch`, lazy destructuring, or statement-position declarations does not apply there.

If you need TSRX-specific control flow or local declarations, keep them in the surrounding component body and use `<tsx>` only for the final JSX expression.

Do not assign or return bare JSX outside `<tsx>`, and do not expect `<tsx>` to accept TSRX-specific syntax.

### Text Containers

Use a single JS or TS expression inside each container. Adjacent containers concatenate.

```tsrx
<p>{'Hello, '}{name}{'!'}</p>
<p>{count > 0 ? 'Unread' : 'All caught up'}</p>
```

Special forms:

- `{text expr}`: force escaped text output

### Lazy Destructuring

Use `&{ ... }` and `&[ ... ]` when bindings should stay linked to the source lookup instead of eagerly snapshotting values.

```tsrx
component UserCard(&{ name, age }: { name: string; age: number }) {
  <div>
    <h2>{name}</h2>
    <p>{`Age: ${age}`}</p>
  </div>
}
```

```tsrx
let &[count, setCount] = createSignal(0);
```

Use lazy destructuring for:

- Component props that are clearer as deferred source-property reads
- Signal-like tuple returns where deferred index access matters

Support the same TS destructuring features the source calls out, including defaults and rest patterns.

### Prop Shorthand

Use prop shorthand when the prop name matches the variable name.

```tsrx
<Input {value} {onChange} />
```

### Refs

Use `{ref variable}` for mutable references or `{ref callback}` for callback refs.

```tsrx
component AutoFocus() {
  let input: HTMLInputElement | undefined;

  <input {ref input} type="text" />
  <input {ref (node) => node.focus()} />
}
```

Refs also work through composite components when the child forwards the ref to DOM.

### Lexical Template Scoping

Treat each nested element body as a lexical scope. Declare locals directly inside element bodies when that keeps computation beside the markup it feeds.

```tsrx
component App() {
  const name = 'World';

  <div>
    const greeting = `Hello, ${name}!`;
    <h1>{greeting}</h1>
  </div>
}
```

Plain statements such as declarations, logging, and `debugger` can appear alongside JSX children.

### Children

Nest JSX for standard composition:

```tsrx
<Card>
  <h2>{'Title'}</h2>
  <p>{'Content goes here.'}</p>
</Card>
```

Pass `children={expr}` when the value is computed:

```tsrx
<List children={items.map(renderItem)} />
```

## Control Flow and Styles

### `if` / `else if` / `else`

Use normal JavaScript conditionals inside templates.

```tsrx
component StatusBadge({ status }: { status: string }) {
  <div>
    if (status === 'active') {
      <span class="badge active">{'Online'}</span>
    } else if (status === 'idle') {
      <span class="badge idle">{'Away'}</span>
    } else {
      <span class="badge">{'Offline'}</span>
    }
  </div>
}
```

### `for ... of` with `index` and `key`

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

### `switch`

Use standard `switch` statements for multi-branch rendering. `break` terminates a case and fall-through still works.

```tsrx
switch (status) {
  case 'loading':
    <p>{'Loading...'}</p>
    break;
  case 'success':
    <p class="success">{'Done!'}</p>
    break;
  default:
    <p>{'Unknown status.'}</p>
}
```

### `try` / `pending` / `catch`

Use `try { ... } catch (e) { ... }` for error fallback UI. Add `pending { ... }` to model async loading boundaries.

```tsrx
const UserProfile = lazy(() => import('./UserProfile.tsrx'));

export component App() {
  try {
    <UserProfile id={1} />
  } pending {
    <p>{'Loading...'}</p>
  } catch (e) {
    <p>{'Something went wrong.'}</p>
  }
}
```

Treat `try`, `pending`, and `catch` as template control-flow constructs, not as ad hoc wrapper components.

### Early Returns

Use a bare `return;` for guard clauses after rendering fallback content. Do not return a value.

```tsrx
component Dashboard({ user }: { user: User | null }) {
  if (!user) {
    <p>{'Please sign in.'}</p>
    return;
  }

  <h1>{`Welcome, ${user.name}`}</h1>
}
```

Do not translate React-style `return (...)` directly into TSRX.

### Scoped Styles

Use a component-local `<style>` block to scope selectors to the component with a unique hash.

```tsrx
component Card() {
  <div class="card">
    <h2>{'Scoped title'}</h2>
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

### `:global(...)`

Use `:global(...)` to opt out of scoping for resets or selectors that must escape the component boundary.

Examples called out by the source:

- `:global(.class-name)`
- `:global(.foo, .bar)`
- `.scoped :global(.unscoped) .also-scoped`

Keep `:global(...)` intentional and narrow.

### `#style`

Use `#style.className` to pass a scoped class name to a child component.

```tsrx
component Badge({ className }: { className?: string }) {
  <span class={`badge ${className ?? ''}`}>{'New'}</span>
  <style>
    .badge { padding: 0.25rem 0.5rem; }
  </style>
}

component App() {
  <Badge className={#style.highlight} />
  <style>
    .highlight { background: #e8f5e9; color: #2e7d32; }
  </style>
}
```

The referenced class must appear as a standalone selector in the surrounding component's `<style>`.

## React Behavior and Common Patterns

### React Behavior

`@tsrx/react` adds behavior that differs from ordinary JSX authoring:

- Lift hook calls to the top of the generated component function, even when the TSRX source places them after a guard or inside a loop.
- Allow top-level component-body `await`.
- Emit React-compatible TSX for the rest of the build pipeline.

Treat these as compile-time conveniences, not as a reason to write opaque code. Keep component flow readable even when the compiler can legalize it.

Do not:

- Mark components `async`
- Use `for await...of` inside component templates
- Translate React early returns into `return <JSX />`

### Common Escape Hatches

Use a plain inline function when you want ordinary JS or TS control flow that should not obey template rules.

```tsrx
export component Counter() {
  let count = 0;

  const increment = () => {
    if (count >= 10) {
      count = 0;
    } else {
      count += 1;
    }
  };

  <button onClick={increment}>
    {`Count: ${count}`}
  </button>
}
```

Use nested JSX for normal composition and `children={expr}` for computed children values.

Keep scoped locals close to the JSX they serve instead of hoisting every intermediate value.

### TypeScript

Treat `.tsrx` as a superset of TypeScript. Props, generics, utility types, and standard imports should work as ordinary TypeScript constructs while the compiler emits React-compatible TSX.

### Pitfall Checklist

- Use `component`, not `function`, for TSRX components.
- Keep text inside `{...}`.
- Use `<tsx>` for JSX in expression position, and keep its contents to standard TSX or JSX.
- Use bare `return;` only as a guard.
- Use top-level `await` only when the surrounding React app expects async component behavior.
- Validate the rendered React behavior before assuming the edit is correct.

## Search Shortcuts

- Search this file for component and expression rules with `rg -n "component|<tsx>|\\{text|&\\{|&\\[|ref|children" packages/tsrx-react/SKILL.md`.
- Search this file for control flow and styling rules with `rg -n "if|for|index|key|switch|pending|catch|return;|<style>|:global|#style" packages/tsrx-react/SKILL.md`.
- Search this file for React behavior and escape hatches with `rg -n "hook|await|for await|async|TypeScript|inline function" packages/tsrx-react/SKILL.md`.
