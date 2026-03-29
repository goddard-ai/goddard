import { render } from "preact"
import App from "./App"
import { desktopHost, initializeDesktopHost } from "./desktop-host"
import "./App.css"

initializeDesktopHost()

// Expose the desktop bridge for manual smoke checks until real flows consume it.
window.__goddardDesktop = desktopHost

render(<App />, document.getElementById("root")!)
