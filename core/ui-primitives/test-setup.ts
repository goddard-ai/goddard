import { GlobalRegistrator } from "@happy-dom/global-registrator"
import tsrxPlugin from "@tsrx/bun-plugin-preact"

Bun.plugin(tsrxPlugin())

GlobalRegistrator.register({
  url: "http://localhost",
})

if (!window.requestAnimationFrame) {
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 16)
}

if (!window.cancelAnimationFrame) {
  window.cancelAnimationFrame = (handle) => window.clearTimeout(handle)
}
