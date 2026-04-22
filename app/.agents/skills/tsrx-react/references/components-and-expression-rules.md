# Components and Expression Rules

Source: derived from `https://tsrx.dev/llms.txt` fetched on April 22, 2026.

## Components

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

## Statement-Based JSX

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

## `<tsx>` for Expression Position

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

Do not assign or return bare JSX outside `<tsx>`.

## Text Containers

Use a single JS or TS expression inside each container. Adjacent containers concatenate.

```tsrx
<p>{'Hello, '}{name}{'!'}</p>
<p>{count > 0 ? 'Unread' : 'All caught up'}</p>
```

Special forms:

- `{text expr}`: force escaped text output

## Lazy Destructuring

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

## Prop Shorthand

Use prop shorthand when the prop name matches the variable name.

```tsrx
<Input {value} {onChange} />
```

## Refs

Use `{ref variable}` for mutable references or `{ref callback}` for callback refs.

```tsrx
component AutoFocus() {
  let input: HTMLInputElement | undefined;

  <input {ref input} type="text" />
  <input {ref (node) => node.focus()} />
}
```

The fetched source says refs also work through composite components when the child forwards the ref to DOM.

## Lexical Template Scoping

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

## Children

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
