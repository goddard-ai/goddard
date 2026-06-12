import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"
import { signal } from "@preact/signals"
import { expect, mock, test } from "bun:test"
import { render } from "preact"
import { act } from "preact/test-utils"

mock.module("lucide-react", () => ({
  Bot: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
  ChevronDown: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
  LoaderCircle: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
}))

const { SessionInputSelect } = await import("./select-menu.tsrx")

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

test("SessionInputSelect focuses the active option when a non-searchable menu opens", async () => {
  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(true)

  document.body.append(container, menuRoot)
  setOverlayPortalRoots({
    menu: menuRoot,
  })

  await act(async () => {
    render(
      <SessionInputSelect
        filterable={false}
        items={[
          { value: "alpha", label: "Alpha" },
          { value: "beta", label: "Beta" },
          { value: "gamma", label: "Gamma" },
        ]}
        label="Mode"
        open={open}
        placeholder="Choose mode"
        value="beta"
        onValueChange={() => {}}
      />,
      container,
    )
  })
  await flushEffects()

  const activeOption = menuRoot.querySelector<HTMLButtonElement>("[data-active='true']")

  expect(activeOption?.textContent).toContain("Beta")
  expect(document.activeElement).toBe(activeOption)

  render(null, container)
})
