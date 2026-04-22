# React Behavior and Common Patterns

Source: derived from `https://tsrx.dev/llms.txt` fetched on April 22, 2026.

## React Behavior

`@tsrx/react` adds behavior that differs from ordinary JSX authoring:

- Lift hook calls to the top of the generated component function, even when the TSRX source places them after a guard or inside a loop.
- Allow top-level component-body `await`.
- Emit React-compatible TSX for the rest of the build pipeline.

Treat these as compile-time conveniences, not as a reason to write opaque code. Keep component flow readable even when the compiler can legalize it.

Do not:

- Mark components `async`
- Use `for await...of` inside component templates
- Translate React early returns into `return <JSX />`

## Common Escape Hatches

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

## TypeScript

Treat `.tsrx` as a superset of TypeScript. Props, generics, utility types, and standard imports should work as ordinary TypeScript constructs while the compiler emits React-compatible TSX.

## Pitfall Checklist

- Use `component`, not `function`, for TSRX components.
- Keep text inside `{...}`.
- Use `<tsx>` for JSX in expression position.
- Use bare `return;` only as a guard.
- Use top-level `await` only when the surrounding React app expects async component behavior.
- Validate the rendered React behavior before assuming the edit is correct.
