---
name: tsrx-preact
description: Author or maintain Preact components in `.tsrx` files, including TSRX component declarations, statement-position markup, scoped templates, render props, and template control flow.
---

## Overview

TSRX is a TypeScript language extension for authoring Preact UI in `.tsrx` files.

Key traits:

- Treat JSX elements as statements rather than expressions.
- Keep control flow directly in the template with `if`, `for`, `switch`, and `try`.
- Keep locals scoped near the JSX that uses them.
- Preserve TypeScript types through the compile step.
- Emit Preact-compatible JSX for the rest of the build pipeline.

## Mental Model

- Treat `.tsrx` as a Preact authoring language, not as plain JSX with a few helpers.
- Keep markup, control flow, and local declarations close together in the component body.
- Prefer local clarity over JSX habits carried over from function components.
- Let TSRX features such as lexical scoping and statement-position JSX do the structural work instead of recreating JSX-era patterns.

## Start Here

- Inspect the current `.tsrx` authoring patterns first: component shape, control flow, hook placement, and local scopes.
- Use the in-file sections below as needed:
  - Read `Components and Expression Rules` before editing component declarations, text, props, children, or expression-position TSRX.
  - Read `Control Flow` before editing `if`, `for`, `switch`, `try` / `pending` / `catch`, or early returns.
  - Read `Preact Behavior and Common Patterns` before relying on Preact hook behavior or common escape-hatch patterns.
- Preserve the existing Preact semantics before abstracting anything.

## Workflow

1. Identify the current Preact authoring pattern and its constraints.
   - Check how components express guards, loops, nested scopes, and computed children.
   - Check how hooks, event handlers, and local declarations are placed.
2. Apply TSRX syntax instead of JSX habits.
   - Define UI building blocks with `component`, `export component`, or component arrow functions.
   - Place JSX elements directly in the component body as statements.
   - Write static JSX text as double-quoted text nodes, not bare text.
   - Use `{...}` for JavaScript expressions, dynamic values, and prop values.
   - Use `<tsrx>...</tsrx>` when TSRX markup must appear in expression position.
   - Use a bare `return;` only to stop later template output after rendering a guard branch.
3. Preserve Preact behavior and safety.
   - Do not `return <JSX />`, `return someValue`, or assign bare JSX to variables outside `<tsrx>`.
   - Do not mark components `async`.
   - Do not use top-level component-body `await`; Preact components are synchronous render functions.
   - Do not use `for await...of` inside Preact component templates.
4. Validate the generated behavior after editing.
   - Check that keyed loops and computed children still behave correctly in the generated Preact code.
   - Check that guard clauses and async boundaries still lower to the intended Preact behavior.
   - Check hook placement and stateful behavior in the compiled component when you changed control flow around hooks.

## Components and Expression Rules

### Components

Define components with `component`, not `function`. Keep the template directly in the component body and do not return JSX.

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

Component arrow functions are also valid when assigning a component to a variable or class field.

```tsrx
const myComponent = component () => {
  <div />
}

class Dialog {
  static Root = component () => {
    <div />
  }
}
```

Prefer these rules:

- Use `component Name(props: Props) { ... }`, `export component Name(...) { ... }`, or `const Name = component (...) => { ... }`.
- Keep JSX statements, handlers, and local declarations in the component body.
- Do not mix plain `function Component()` and TSRX `component` styles in the same TSRX code.
- Do not `return <JSX />` from a component body.

### Statement-Based JSX

Treat JSX as statements, not expressions. Static text still needs double quotes, but it no longer needs an expression container.

```tsrx
component Greeting() {
  const count = 3;

  <h1>"Hello World"</h1>
  <p>"Count: "{count}</p>
}
```

Do not write bare text:

```tsrx
<div>Hello World</div>
```

Do not keep old JSX expression containers for static text:

```tsrx
<span>{"Foo"}</span>
```

Write this instead:

```tsrx
<span>"Foo"</span>
```

### Expression-Position TSRX

Use `<tsrx>...</tsrx>` when markup must live in expression position, such as assignment, helper returns, or prop values.

```tsrx
component App() {
  const title = <tsrx><span class="title">"Settings"</span></tsrx>;
  <Card {title} />
}
```

Use `<tsrx>` when:

- Assigning TSRX markup to a variable
- Returning markup from a plain helper function
- Passing markup through a prop value
- Using render props or function-as-children callbacks that return markup

Because `<tsrx>` uses TSRX syntax, keep its contents consistent with the rest of the file: statement-style markup, double-quoted static text, and TSRX template control flow where needed.

`<tsrx>` is mandatory for render-prop patterns because the callback returns markup in expression position.

```tsrx
<List
  {items}
  renderItem={(item) => <tsrx><li key={item.id}>{item.label}</li></tsrx>}
/>
```

Do not assign or return bare JSX outside `<tsrx>`.

### Text Containers

Use double-quoted text nodes for static text. Use expression containers for JavaScript expressions, including variables, conditionals, and template strings. In TSRX blocks, quoted text nodes can sit directly next to expression containers, so `"string"{value}` is valid JSX text.

```tsrx
<p>"Hello, "{name}"!"</p>
<p>{count > 0 ? 'Unread' : 'All caught up'}</p>
```

Special forms:

- `{text expr}`: force escaped text output

### Prop Shorthand

Use prop shorthand when the prop name matches the variable name.

```tsrx
<Input {value} {onInput} />
```

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
  <h2>"Title"</h2>
  <p>"Content goes here."</p>
</Card>
```

Pass `children={expr}` when the value is computed:

```tsrx
<List children={items.map(renderItem)} />
```

## Control Flow

### `if` / `else if` / `else`

Use normal JavaScript conditionals inside templates.

```tsrx
component StatusBadge({ status }: { status: string }) {
  <div>
    if (status === 'active') {
      <span class="badge active">"Online"</span>
    } else if (status === 'idle') {
      <span class="badge idle">"Away"</span>
    } else {
      <span class="badge">"Offline"</span>
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
    <p>"Loading..."</p>
    break;
  case 'success':
    <p class="success">"Done!"</p>
    break;
  default:
    <p>"Unknown status."</p>
}
```

### `try` / `pending` / `catch`

Use `try { ... } catch (e) { ... }` for error fallback UI. Add `pending { ... }` to model async loading boundaries around child components or resources. A `catch` block may receive a retry callback as its second parameter, `catch (error, retry) { ... }`, and UI can call `retry()` to rerender the failed boundary after any needed cache invalidation or local cleanup.

```tsrx
const UserProfile = lazy(() => import('./UserProfile.tsrx'));

export component App() {
  try {
    <UserProfile id={1} />
  } pending {
    <p>"Loading..."</p>
  } catch (e, retry) {
    <p>"Something went wrong."</p>
    <button onClick={retry}>
      "Try again"
    </button>
  }
}
```

Treat `try`, `pending`, and `catch` as template control-flow constructs, not as ad hoc wrapper components.

### Early Returns

Use a bare `return;` for guard clauses after rendering fallback content. Do not return a value.

```tsrx
component Dashboard({ user }: { user: User | null }) {
  if (!user) {
    <p>"Please sign in."</p>
    return;
  }

  <h1>"Welcome, "{user.name}</h1>
}
```

Do not translate JSX-style `return (...)` directly into TSRX.

## Preact Behavior and Common Patterns

### Preact Behavior

`@tsrx/preact` emits Preact-compatible JSX while keeping TSRX's statement-based component syntax.

Treat TSRX syntax as a compile-time authoring layer over ordinary Preact component behavior:

- Components must remain synchronous; do not use `async component` or top-level component-body `await`.
- Use Preact event names and conventions such as `onInput` for text inputs.
- Keep Preact hooks readable and predictable when editing control flow around stateful logic.

Do not:

- Mark components `async`
- Use `for await...of` inside component templates
- Translate JSX early returns into `return <JSX />`

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
    "Count: "{count}
  </button>
}
```

Use nested JSX for normal composition and `children={expr}` for computed children values.

Keep scoped locals close to the JSX they serve instead of hoisting every intermediate value.

### TypeScript

Treat `.tsrx` as a superset of TypeScript. Props, generics, utility types, and standard imports should work as ordinary TypeScript constructs while the compiler emits Preact-compatible JSX.

### Pitfall Checklist

- Use `component` declarations or component arrow functions, not plain `function`, for TSRX components.
- Write static text as double-quoted text nodes, such as `<span>"Foo"</span>`.
- Use `{...}` for JavaScript expressions and dynamic values.
- Use `<tsrx>` for TSRX markup in expression position, including render-prop callbacks.
- Use bare `return;` only as a guard.
- Do not use top-level `await` in component bodies; Preact does not support async components.
- Validate the rendered Preact behavior before assuming the edit is correct.
