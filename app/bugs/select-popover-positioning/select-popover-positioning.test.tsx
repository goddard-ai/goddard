import { Popover } from "@ark-ui/react/popover"
import { Portal } from "@ark-ui/react/portal"
import { render } from "preact"
import { useRef, useState } from "preact/hooks"
import { afterEach, describe, expect, it, vi } from "vitest"

function ReproMenuPortal(props: { children?: preact.ComponentChildren }) {
  const containerRef = useRef<HTMLElement | null>(document.getElementById("menu-portal"))
  containerRef.current = document.getElementById("menu-portal")

  if (!containerRef.current) {
    return null
  }

  return <Portal container={containerRef}>{props.children}</Portal>
}

function PopoverPositioningRepro(props: { lazyMount: boolean }) {
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const [open, setOpen] = useState(false)

  return (
    <div>
      <Popover.Root
        lazyMount={props.lazyMount}
        open={open}
        positioning={{
          getAnchorElement: () => triggerRef.current,
          placement: "bottom-start",
          strategy: "fixed",
        }}
        unmountOnExit={props.lazyMount}
        onOpenChange={(details) => {
          setOpen(details.open)
        }}
      >
        <Popover.Trigger asChild>
          <button ref={triggerRef} data-repro-role="trigger" type="button">
            Trigger
          </button>
        </Popover.Trigger>

        <button
          data-repro-role="open"
          type="button"
          onClick={() => {
            setOpen(true)
          }}
        >
          Open
        </button>

        <ReproMenuPortal>
          <Popover.Positioner>
            <Popover.Content>Popover content</Popover.Content>
          </Popover.Positioner>
        </ReproMenuPortal>
      </Popover.Root>
    </div>
  )
}

function readPositioningVariables(positioner: HTMLElement) {
  return {
    x: positioner.style.getPropertyValue("--x"),
    y: positioner.style.getPropertyValue("--y"),
    zIndex: positioner.style.getPropertyValue("--z-index"),
  }
}

function getPositioner() {
  return document.querySelector<HTMLElement>('[data-scope="popover"][data-part="positioner"]')
}

function renderRepro(lazyMount: boolean) {
  const root = document.getElementById("root")

  if (!root) {
    throw new Error("Expected a #root element in the test document.")
  }

  render(<PopoverPositioningRepro lazyMount={lazyMount} />, root)

  return {
    openButton: document.querySelector<HTMLButtonElement>('[data-repro-role="open"]'),
  }
}

async function flushPopover() {
  await Promise.resolve()
  await new Promise((resolve) => window.setTimeout(resolve, 0))
  await Promise.resolve()
}

afterEach(() => {
  const root = document.getElementById("root")

  if (root) {
    render(null, root)
  }
})

describe("Ark popover positioning", () => {
  it("leaves positioning variables unset when the positioner is lazy-mounted", async () => {
    const { openButton } = renderRepro(true)

    if (!openButton) {
      throw new Error("Expected an open button.")
    }

    openButton.click()

    await vi.waitFor(() => {
      expect(getPositioner()).toBeTruthy()
    })

    await flushPopover()

    expect(readPositioningVariables(getPositioner()!)).toEqual({
      x: "",
      y: "",
      zIndex: "",
    })
  })

  it("sets positioning variables when the positioner stays mounted", async () => {
    const { openButton } = renderRepro(false)

    if (!openButton) {
      throw new Error("Expected an open button.")
    }

    openButton.click()

    await vi.waitFor(() => {
      const positioner = getPositioner()
      const variables = positioner ? readPositioningVariables(positioner) : null

      expect(positioner).toBeTruthy()
      expect(variables?.x).not.toBe("")
      expect(variables?.y).not.toBe("")
      expect(variables?.zIndex).not.toBe("")
    })
  })
})
