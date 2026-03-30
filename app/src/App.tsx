import { AppShell } from "./components/AppShell"
import { AppStateProvider } from "./components/state/AppStateContext"

/** Boots the app shell within the shared app-state context providers. */
function App() {
  return (
    <AppStateProvider>
      <AppShell />
    </AppStateProvider>
  )
}

export default App
