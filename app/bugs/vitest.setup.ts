import { beforeEach } from "vitest"

function createRect(x: number, y: number, width: number, height: number): DOMRect {
  return {
    bottom: y + height,
    height,
    left: x,
    right: x + width,
    top: y,
    width,
    x,
    y,
    toJSON() {
      return {
        bottom: y + height,
        height,
        left: x,
        right: x + width,
        top: y,
        width,
        x,
        y,
      }
    },
  } as DOMRect
}

Object.defineProperty(window, "innerHeight", {
  configurable: true,
  value: 768,
})

Object.defineProperty(window, "innerWidth", {
  configurable: true,
  value: 1024,
})

Object.defineProperty(globalThis, "ResizeObserver", {
  configurable: true,
  value: class ResizeObserver {
    disconnect() {}

    observe() {}

    unobserve() {}
  },
})

Object.defineProperty(globalThis, "IntersectionObserver", {
  configurable: true,
  value: class IntersectionObserver {
    disconnect() {}

    observe() {}

    unobserve() {}

    takeRecords() {
      return []
    }
  },
})

Object.defineProperty(window, "requestAnimationFrame", {
  configurable: true,
  value(callback: FrameRequestCallback) {
    return window.setTimeout(() => callback(Date.now()), 0)
  },
})

Object.defineProperty(window, "cancelAnimationFrame", {
  configurable: true,
  value(handle: number) {
    window.clearTimeout(handle)
  },
})

Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
  configurable: true,
  get() {
    return Math.round(this.getBoundingClientRect().height)
  },
})

Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
  configurable: true,
  get() {
    return Math.round(this.getBoundingClientRect().width)
  },
})

Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value() {},
})

Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
  configurable: true,
  value() {
    if (this.id === "menu-portal") {
      return createRect(0, 0, 1024, 768)
    }

    if (this instanceof HTMLElement && this.dataset.reproRole === "trigger") {
      return createRect(120, 100, 180, 32)
    }

    if (
      this instanceof HTMLElement &&
      (this.getAttribute("data-part") === "positioner" ||
        this.getAttribute("data-part") === "content")
    ) {
      return createRect(0, 0, 180, 240)
    }

    return createRect(0, 0, 0, 0)
  },
})

beforeEach(() => {
  document.body.innerHTML = `
    <div id="root"></div>
    <div id="menu-portal"></div>
  `
})
