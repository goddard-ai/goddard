# Component: DeviceFlowDialog

- **Minimum Viable Component:** Modal dialog for GitHub device-flow authentication that shows the verification URL, user code, status, and completion progress.
- **Props Interface:** `isOpen: boolean`; `deviceFlow: { verificationUri, userCode, expiresAt, pollIntervalSeconds } | null`; `status: "idle" | "starting" | "waiting" | "authorized" | "error"`; `errorMessage?: string | null`; `onClose: () => void`; `onRetry: () => void`.
- **Sub-components:** None.
- **State Complexity:** Simple UI-only countdown and copy-confirmation state; device-flow lifecycle belongs in `AuthState`.
- **Required Context:** `AuthContext`.
- **Electrobun RPC:** None directly; all auth requests route through `AuthState`.
- **Interactions & Events:** Opens the browser verification flow; copies the code; shows polling progress; closes or retries when the flow expires or fails.
