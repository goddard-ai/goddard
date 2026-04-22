# Composition and Refs

## `asChild`

Use `asChild` when an Ark part should attach its behavior to your own component instead of rendering its default element.

```tsx
<Popover.Trigger asChild>
  <Button>Open</Button>
</Popover.Trigger>
```

Keep these rules:

- Pass exactly one child element.
- Make the child spread received props.
- Forward the ref to the rendered DOM element.
- Preserve the semantics of the rendered element so the trigger stays interactive and accessible.

Some components have stricter child requirements, so inspect the local implementation when composition behaves strangely.

## `ark` Factory

Use the `ark` factory when you need your own elements that still behave like Ark UI elements.

## Shared `ids`

Share `ids` across composed components when multiple primitives need to refer to the same DOM identity.

```tsx
import { Avatar } from "@ark-ui/react/avatar";
import { Tooltip } from "@ark-ui/react/tooltip";
import { useId } from "react";

export const TooltipWithAvatar = () => {
  const id = useId();

  return (
    <Tooltip.Root ids={{ trigger: id }}>
      <Tooltip.Trigger asChild>
        <Avatar.Root ids={{ root: id }}>
          <Avatar.Image src="..." />
          <Avatar.Fallback>SA</Avatar.Fallback>
        </Avatar.Root>
      </Tooltip.Trigger>
      <Tooltip.Positioner>
        <Tooltip.Content>Segun Adebayo is online</Tooltip.Content>
      </Tooltip.Positioner>
    </Tooltip.Root>
  );
};
```

## Refs

Attach refs to the rendered part when parent code needs measurement, focus management, or imperative DOM access.

```tsx
import { Slider } from "@ark-ui/react/slider";
import { useRef } from "react";

export const MySlider = () => {
  const rootRef = useRef<HTMLDivElement | null>(null);
  return <Slider.Root ref={rootRef}>{/* ... */}</Slider.Root>;
};
```
