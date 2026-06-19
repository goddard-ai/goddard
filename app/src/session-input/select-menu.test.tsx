import { setOverlayPortalRoots } from "@goddard-ai/ui-primitives"
import { signal } from "@preact/signals"
import { render } from "preact"
import { act } from "preact/test-utils"
import { expect, test, vi } from "vitest"

vi.mock("lucide-react", () => ({
  Bot: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
  ChevronDown: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
  LoaderCircle: (props: preact.SVGAttributes<SVGSVGElement>) => <svg {...props} />,
}))

let captureFocusedPrompt = false
let captureFocusedPromptCalls = 0
let restoreFocusedPromptCalls = 0

vi.mock("./prompt-focus.ts", () => ({
  captureFocusedSessionInputPrompt() {
    captureFocusedPromptCalls += 1

    if (!captureFocusedPrompt) {
      return null
    }

    return () => {
      restoreFocusedPromptCalls += 1
    }
  },
  registerSessionInputPromptFocus() {
    return () => {}
  },
}))

const { SessionInputSelect } = await import("./select-menu.tsrx")

async function flushEffects() {
  await act(async () => {
    await Promise.resolve()
  })
}

test("SessionInputSelect focuses the active option when a non-searchable menu opens", async () => {
  captureFocusedPrompt = false
  captureFocusedPromptCalls = 0
  restoreFocusedPromptCalls = 0

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

test("SessionInputSelect restores a focused prompt after the menu closes", async () => {
  captureFocusedPrompt = true
  captureFocusedPromptCalls = 0
  restoreFocusedPromptCalls = 0

  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(false)

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
        ]}
        label="Mode"
        open={open}
        placeholder="Choose mode"
        value="alpha"
        onValueChange={() => {}}
      />,
      container,
    )
  })

  const trigger = container.querySelector<HTMLButtonElement>("button[aria-label='Mode: Alpha']")

  await act(async () => {
    trigger?.click()
  })
  await flushEffects()

  const betaOption = Array.from(menuRoot.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.includes("Beta"),
  )

  await act(async () => {
    betaOption?.click()
  })
  await flushEffects()

  expect(captureFocusedPromptCalls).toBe(1)
  expect(restoreFocusedPromptCalls).toBe(1)

  render(null, container)
})

test("SessionInputSelect restores the trigger after selection when no prompt was focused", async () => {
  captureFocusedPrompt = false
  captureFocusedPromptCalls = 0
  restoreFocusedPromptCalls = 0

  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const open = signal(false)

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
        ]}
        label="Mode"
        open={open}
        placeholder="Choose mode"
        value="alpha"
        onValueChange={() => {}}
      />,
      container,
    )
  })

  const trigger = container.querySelector<HTMLButtonElement>("button[aria-label='Mode: Alpha']")

  await act(async () => {
    trigger?.click()
  })
  await flushEffects()

  const betaOption = Array.from(menuRoot.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.includes("Beta"),
  )

  await act(async () => {
    betaOption?.click()
  })
  await flushEffects()

  expect(document.activeElement).toBe(trigger)
  expect(restoreFocusedPromptCalls).toBe(0)

  render(null, container)
})

test("SessionInputSelect hides when focus leaves the menu", async () => {
  captureFocusedPrompt = false
  captureFocusedPromptCalls = 0
  restoreFocusedPromptCalls = 0

  const container = document.createElement("div")
  const menuRoot = document.createElement("div")
  const outsideButton = document.createElement("button")
  const open = signal(true)

  document.body.append(container, menuRoot, outsideButton)
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
        ]}
        label="Mode"
        open={open}
        placeholder="Choose mode"
        value="alpha"
        onValueChange={() => {}}
      />,
      container,
    )
  })
  await flushEffects()

  const activeOption = menuRoot.querySelector<HTMLButtonElement>("[data-active='true']")

  expect(document.activeElement).toBe(activeOption)

  await act(async () => {
    outsideButton.focus()
  })
  await flushEffects()

  expect(open.value).toBe(false)
  expect(document.activeElement).toBe(outsideButton)

  render(null, container)
  menuRoot.remove()
  outsideButton.remove()
})
