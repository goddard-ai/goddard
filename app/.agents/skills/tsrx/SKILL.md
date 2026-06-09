---
name: tsrx
description: Use when writing, editing, or reviewing TSRX (.tsrx)
---

# TSRX Syntax Mechanics

TSRX is TypeScript + JSX with opt-in syntax for JSX-producing templates. Existing TypeScript and JSX keep their meanings, with native `<style>` blocks parsed as CSS. To render syntax markers as text, use JSX string expressions like `{'@if'}`.

For legacy TSRX migration notes, read `references/migration.md`.

## Template Result Rule

`@{ ... }` creates a local scope whose final statement produces JSX.

```tsx
@{
  const value = compute();
  <View value={value} />
}
```

As a JSX child, mirror formatter shape:

```tsx
<div>@{
  const value = compute();
  <View value={value} />
}</div>
```

Final JSX-producing forms: JSX element, JSX fragment, `@if`, `@for`, `@switch`, `@try`. Wrap final text or expression values in a fragment: `<>{label}</>`.

`return` is available only in top-level TSRX function bodies. Other TSRX blocks yield their final JSX-producing statement.

Use templates as expressions, directly inside JSX layout, or as function bodies.

## JSX Conveniences

- JS comments may appear directly in JSX: `// ...` and `/* ... */`.
- In JSX attribute position, `{foo}` means `foo={foo}`.

## `@if`

```tsx
@if (condition) {
  const value = compute();
  <A value={value} />
} @else if (other) {
  <B />
} @else {
  <Fallback />
}
```

Each branch may run local JS statements, then finishes with JSX. The matching branch supplies the output.

## `@for` / `@empty`

```tsx
@for (const item of items; index i; key item.id) {
  <Item item={item} index={i} />
} @empty {
  <Empty />
}
```

The suffix after `;` may declare `index`, `key`, or `index` then `key`. The `index` variable is in scope for `key`. A `key` is propagated to rendered elements, including through shorthand fragments. `@empty` renders for empty iterables.

Each iteration may run local JS statements, then finishes with JSX. `break` and `continue` are excluded from `@for` bodies.

## `@switch`

```tsx
@switch (value) {
  @case 'a': {
    <A />
  }
  @case 'b':
  @case 'c': {
    <BC />
  }
  @default: {
    <Fallback />
  }
}
```

`@case` and `@default` use trailing `:`. Stacked `@case` labels share the next block. The selected branch supplies the output; `break` is omitted.

## `@try` / `@pending` / `@catch`

```tsx
@try {
  <Content />
} @pending {
  <Loading />
} @catch (error, reset) {
  <ErrorState error={error} onRetry={reset} />
}
```

`@try` protects the main render tree, `@pending` supplies async fallback UI, and `@catch` supplies error UI. `@catch` receives `error` and optionally `reset`. Each block finishes with JSX; emitted boundaries are target-specific.

Use empty `@pending {}` or `@catch (...) {}` blocks when there is no fallback or error UI; do not add an empty fragment solely to satisfy the block.

## Function Bodies

```tsx
function Component(props) @{
  if (props.hidden) return null;
  const value = compute(props);
  <View value={value} />
}

const Component = (props) => @{
  <View {...props} />
}
```

Function declarations and arrow functions may use `@{ ... }` bodies. Top-level early `return` exits the function; otherwise the final JSX-producing statement is the output. In fine-grained targets, guard clauses can compile to reactive control flow.

## Conditional Hooks

Hooks may be authored inside conditionals, loops, switches, after early returns, and extractable TSRX templates. The compiler extracts hook-containing paths into target-safe internal components while preserving captured values.

## Native `<style>`

JSX-child style blocks contain direct CSS, are extracted, scoped, and render as `null`.

```tsx
<div>
  <h1>Title</h1>
  <style>
    h1 { color: coral; }
    .box { padding: 1rem; }
  </style>
</div>
```

Variable-initializer style blocks create class maps; class selectors become typed properties. This form works locally or at module scope.

```tsx
const styles = <style>
  .card { border: 1px solid #ccc; }
</style>;

<div className={styles.card} />
```

Use `:global(...)` for intentional global selectors. For runtime values, put CSS custom properties on JSX elements and read them inside CSS.

## Lazy Destructuring `&`

Use `&` before object or array destructuring to preserve fine-grained reactivity in Solid, Vue, and Ripple targets.

```tsx
function UserCard(&{ user, theme }) @{
  const &[count, setCount] = user.counter;
  const &{ displayName, ...details } = user;
  <article className={theme} title={details.title}>{displayName}: {count}</article>
}
```

Valid anywhere standard destructuring is valid: parameters, local declarations, nested scopes, objects, arrays, and rest patterns. Object rest preserves property descriptors.
