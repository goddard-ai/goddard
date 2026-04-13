import { GlobalRegistrator } from "@happy-dom/global-registrator"
import { afterEach } from "bun:test"

GlobalRegistrator.register({
  url: "http://localhost",
})

const testWindow = window as Window & {
  __electrobunWebviewId?: string
  __electrobunRpcSocketPort?: number
  __electrobun?: {
    receiveMessageFromBun?: (message: unknown) => void
  }
  __electrobunBunBridge?: {
    postMessage(message: string): void
  }
  __electrobun_encrypt?: (message: string) => Promise<{
    encryptedData: string
    iv: string
    tag: string
  }>
  __electrobun_decrypt?: (encryptedData: string, iv: string, tag: string) => Promise<string>
}

testWindow.__electrobunWebviewId = "test"
testWindow.__electrobunRpcSocketPort = 0
testWindow.__electrobun ??= {}
testWindow.__electrobunBunBridge ??= {
  postMessage() {},
}
testWindow.__electrobun_encrypt ??= async (message: string) => ({
  encryptedData: message,
  iv: "",
  tag: "",
})
testWindow.__electrobun_decrypt ??= async (encryptedData: string) => encryptedData

if (!window.matchMedia) {
  window.matchMedia = (media) => ({
    matches: false,
    media,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent() {
      return false
    },
  })
}

if (!window.requestAnimationFrame) {
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(Date.now()), 16)
}

if (!window.cancelAnimationFrame) {
  window.cancelAnimationFrame = (handle) => window.clearTimeout(handle)
}

afterEach(() => {
  document.head.innerHTML = ""
  document.body.innerHTML = ""
  document.title = ""
  window.localStorage.clear()
  window.sessionStorage.clear()
})
