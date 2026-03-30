import { AppShell } from "./components/AppShell"
import { AppStateProvider } from "./state/app-context"

/** Boots the sprint-1 shell within the shared app-state context providers. */
function App() {
  return (
    <AppStateProvider>
      <AppShell />
    </AppStateProvider>
  )
}

export default App
